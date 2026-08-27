import { DatabaseSync } from "node:sqlite";
import { randomUUID } from "node:crypto";
import type { Lot, Platform } from "./domain/types.js";

const PLATFORM_COLUMN: Readonly<Record<Platform, "posted_bluesky" | "posted_twitter">> = {
  bluesky: "posted_bluesky",
  twitter: "posted_twitter"
};

export interface AuditSummary {
  integrity: string;
  total: number;
  blueskyConfirmed: number;
  twitterConfirmed: number;
  firstBlueskyId: string | null;
  lastBlueskyId: string | null;
  skippedBeforeBlueskyStart: number;
  gapsInBlueskyRun: number;
  remainingAfterBlueskyCursor: number;
  nextByPlatform: Partial<Record<Platform, Lot | null>>;
}

export interface Delivery {
  state: string;
  deterministicKey: string | null;
}

export class LotsDatabase {
  readonly connection: DatabaseSync;

  constructor(path: string, options: { readOnly?: boolean } = {}) {
    this.connection = new DatabaseSync(path, {
      readOnly: options.readOnly ?? false,
      enableForeignKeyConstraints: true
    });
    this.connection.exec("PRAGMA busy_timeout = 5000");
    this.assertLegacySchema();
  }

  close(): void {
    this.connection.close();
  }

  private assertLegacySchema(): void {
    const columns = this.connection.prepare("PRAGMA table_info(lots)").all() as Array<Record<string, unknown>>;
    const names = new Set(columns.map((row) => String(row.name)));
    for (const required of ["id", "address", "lat", "lon", "posted_twitter", "posted_bluesky"]) {
      if (!names.has(required)) throw new Error(`Database is missing required lots.${required} column`);
    }
  }

  migrate(): void {
    this.connection.exec(`
      BEGIN IMMEDIATE;
      CREATE TABLE IF NOT EXISTS schema_migrations (
        version INTEGER PRIMARY KEY,
        applied_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS platform_state (
        platform TEXT PRIMARY KEY CHECK (platform IN ('bluesky', 'twitter')),
        start_after_id TEXT,
        updated_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS post_deliveries (
        lot_id TEXT NOT NULL REFERENCES lots(id),
        platform TEXT NOT NULL CHECK (platform IN ('bluesky', 'twitter')),
        state TEXT NOT NULL CHECK (state IN ('publishing', 'confirmed', 'failed', 'unknown')),
        deterministic_key TEXT,
        post_ref TEXT,
        attempt_count INTEGER NOT NULL DEFAULT 0,
        last_error TEXT,
        started_at TEXT,
        confirmed_at TEXT,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (lot_id, platform)
      );
      CREATE TABLE IF NOT EXISTS bot_runs (
        run_id TEXT PRIMARY KEY,
        application_version TEXT NOT NULL,
        selected_lot_id TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        outcome TEXT,
        error_code TEXT
      );
      CREATE TABLE IF NOT EXISTS bot_leases (
        name TEXT PRIMARY KEY,
        owner TEXT NOT NULL,
        expires_at INTEGER NOT NULL
      );
      INSERT OR IGNORE INTO schema_migrations(version, applied_at)
      VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
      COMMIT;
    `);
  }

  setPlatformStart(platform: Platform, startAfterId: string): void {
    this.connection.prepare(`
      INSERT INTO platform_state(platform, start_after_id, updated_at)
      VALUES (?, ?, ?)
      ON CONFLICT(platform) DO UPDATE SET
        start_after_id = excluded.start_after_id,
        updated_at = excluded.updated_at
    `).run(platform, startAfterId, new Date().toISOString());
  }

  getHighWater(platform: Platform, fallbackStart?: string): string | null {
    const column = PLATFORM_COLUMN[platform];
    const hasStateTable = this.connection.prepare("SELECT 1 present FROM sqlite_master WHERE type = 'table' AND name = 'platform_state'").get() !== undefined;
    const row = hasStateTable
      ? this.connection.prepare(`
          SELECT COALESCE(
            (SELECT MAX(id) FROM lots WHERE ${column} != '0'),
            (SELECT start_after_id FROM platform_state WHERE platform = ?)
          ) AS id
        `).get(platform) as { id: string | null } | undefined
      : this.connection.prepare(`SELECT MAX(id) AS id FROM lots WHERE ${column} != '0'`).get() as { id: string | null } | undefined;
    return row?.id ?? fallbackStart ?? null;
  }

  getNextForPlatform(platform: Platform, fallbackStart?: string): Lot | null {
    const column = PLATFORM_COLUMN[platform];
    const highWater = this.getHighWater(platform, fallbackStart) ?? "0";
    const row = this.connection.prepare(`
      SELECT id, address, lat, lon, posted_twitter, posted_bluesky
      FROM lots
      WHERE id > ? AND ${column} = '0'
      ORDER BY id ASC
      LIMIT 1
    `).get(highWater);
    return row === undefined ? null : row as unknown as Lot;
  }

  getSpecific(id: string): Lot | null {
    const row = this.connection.prepare(`
      SELECT id, address, lat, lon, posted_twitter, posted_bluesky
      FROM lots WHERE id = ? LIMIT 1
    `).get(id);
    return row === undefined ? null : row as unknown as Lot;
  }

  selectNext(platforms: readonly Platform[], specificId?: string, fallbackStarts: Partial<Record<Platform, string>> = {}): { lot: Lot; platforms: Platform[] } | null {
    if (specificId !== undefined) {
      const lot = this.getSpecific(specificId);
      if (lot === null) return null;
      const pending = platforms.filter((platform) => lot[PLATFORM_COLUMN[platform]] === "0");
      return pending.length === 0 ? null : { lot, platforms: pending };
    }

    const candidates = platforms
      .map((platform) => ({ platform, lot: this.getNextForPlatform(platform, fallbackStarts[platform]) }))
      .filter((candidate): candidate is { platform: Platform; lot: Lot } => candidate.lot !== null)
      .sort((left, right) => left.lot.id.localeCompare(right.lot.id));
    const selected = candidates[0];
    if (selected === undefined) return null;
    const selectedId = selected.lot.id;
    return {
      lot: selected.lot,
      platforms: candidates.filter((candidate) => candidate.lot.id === selectedId).map((candidate) => candidate.platform)
    };
  }

  acquireLease(name: string, ttlSeconds: number): string | null {
    const owner = randomUUID();
    const now = Math.floor(Date.now() / 1000);
    const expires = now + ttlSeconds;
    this.connection.exec("BEGIN IMMEDIATE");
    try {
      this.connection.prepare("DELETE FROM bot_leases WHERE name = ? AND expires_at <= ?").run(name, now);
      const result = this.connection.prepare("INSERT OR IGNORE INTO bot_leases(name, owner, expires_at) VALUES (?, ?, ?)").run(name, owner, expires);
      this.connection.exec("COMMIT");
      return result.changes === 1 ? owner : null;
    } catch (error) {
      this.connection.exec("ROLLBACK");
      throw error;
    }
  }

  releaseLease(name: string, owner: string): void {
    this.connection.prepare("DELETE FROM bot_leases WHERE name = ? AND owner = ?").run(name, owner);
  }

  startRun(version: string): string {
    const runId = randomUUID();
    this.connection.prepare("INSERT INTO bot_runs(run_id, application_version, started_at) VALUES (?, ?, ?)")
      .run(runId, version, new Date().toISOString());
    return runId;
  }

  setRunLot(runId: string, lotId: string): void {
    this.connection.prepare("UPDATE bot_runs SET selected_lot_id = ? WHERE run_id = ?").run(lotId, runId);
  }

  finishRun(runId: string, outcome: string, errorCode?: string): void {
    this.connection.prepare("UPDATE bot_runs SET completed_at = ?, outcome = ?, error_code = ? WHERE run_id = ?")
      .run(new Date().toISOString(), outcome, errorCode ?? null, runId);
  }

  beginDelivery(lotId: string, platform: Platform, deterministicKey?: string): void {
    const now = new Date().toISOString();
    this.connection.prepare(`
      INSERT INTO post_deliveries(lot_id, platform, state, deterministic_key, attempt_count, started_at, updated_at)
      VALUES (?, ?, 'publishing', ?, 1, ?, ?)
      ON CONFLICT(lot_id, platform) DO UPDATE SET
        state = 'publishing',
        deterministic_key = excluded.deterministic_key,
        attempt_count = post_deliveries.attempt_count + 1,
        last_error = NULL,
        started_at = excluded.started_at,
        updated_at = excluded.updated_at
    `).run(lotId, platform, deterministicKey ?? null, now, now);
  }

  confirmDelivery(lotId: string, platform: Platform, postRef: string): void {
    const now = new Date().toISOString();
    const column = PLATFORM_COLUMN[platform];
    this.connection.exec("BEGIN IMMEDIATE");
    try {
      this.connection.prepare(`UPDATE lots SET ${column} = ? WHERE id = ? AND ${column} = '0'`).run(postRef, lotId);
      this.connection.prepare(`
        UPDATE post_deliveries
        SET state = 'confirmed', post_ref = ?, confirmed_at = ?, updated_at = ?, last_error = NULL
        WHERE lot_id = ? AND platform = ?
      `).run(postRef, now, now, lotId, platform);
      this.connection.exec("COMMIT");
    } catch (error) {
      this.connection.exec("ROLLBACK");
      throw error;
    }
  }

  failDelivery(lotId: string, platform: Platform, error: unknown, uncertain: boolean): void {
    const message = error instanceof Error ? error.message : String(error);
    this.connection.prepare(`
      UPDATE post_deliveries SET state = ?, last_error = ?, updated_at = ?
      WHERE lot_id = ? AND platform = ?
    `).run(uncertain ? "unknown" : "failed", message.slice(0, 2000), new Date().toISOString(), lotId, platform);
  }

  getDeliveryState(lotId: string, platform: Platform): string | null {
    return this.getDelivery(lotId, platform)?.state ?? null;
  }

  getDelivery(lotId: string, platform: Platform): Delivery | null {
    const row = this.connection.prepare(`
      SELECT state, deterministic_key
      FROM post_deliveries
      WHERE lot_id = ? AND platform = ?
    `).get(lotId, platform) as { state: string; deterministic_key: string | null } | undefined;
    return row === undefined ? null : { state: row.state, deterministicKey: row.deterministic_key };
  }

  audit(platforms: readonly Platform[], fallbackStarts: Partial<Record<Platform, string>> = {}): AuditSummary {
    const integrityRow = this.connection.prepare("PRAGMA integrity_check").get() as Record<string, unknown>;
    const integrity = String(Object.values(integrityRow)[0]);
    const counts = this.connection.prepare(`
      SELECT COUNT(*) total,
             SUM(posted_bluesky NOT IN ('0', '1')) bluesky_confirmed,
             SUM(posted_twitter != '0') twitter_confirmed,
             MIN(CASE WHEN posted_bluesky != '0' THEN id END) first_bluesky,
             MAX(CASE WHEN posted_bluesky != '0' THEN id END) last_bluesky
      FROM lots
    `).get() as Record<string, number | string | null>;
    const first = counts.first_bluesky === null ? null : String(counts.first_bluesky);
    const last = counts.last_bluesky === null ? null : String(counts.last_bluesky);
    const ranges = first === null || last === null ? { skipped: 0, gaps: 0, remaining: Number(counts.total) } :
      this.connection.prepare(`
        SELECT SUM(id < ? AND posted_bluesky = '0') skipped,
               SUM(id BETWEEN ? AND ? AND posted_bluesky = '0') gaps,
               SUM(id > ? AND posted_bluesky = '0') remaining
        FROM lots
      `).get(first, first, last, last) as { skipped: number; gaps: number; remaining: number };
    const nextByPlatform: Partial<Record<Platform, Lot | null>> = {};
    for (const platform of platforms) nextByPlatform[platform] = this.getNextForPlatform(platform, fallbackStarts[platform]);
    return {
      integrity,
      total: Number(counts.total),
      blueskyConfirmed: Number(counts.bluesky_confirmed),
      twitterConfirmed: Number(counts.twitter_confirmed),
      firstBlueskyId: first,
      lastBlueskyId: last,
      skippedBeforeBlueskyStart: Number(ranges.skipped),
      gapsInBlueskyRun: Number(ranges.gaps),
      remainingAfterBlueskyCursor: Number(ranges.remaining),
      nextByPlatform
    };
  }
}
