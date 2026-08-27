import type { AppConfig } from "./config.js";
import { LotsDatabase } from "./db.js";
import type { Logger } from "./logging.js";

const SOURCE_URL = "https://datacatalog.cookcountyil.gov/resource/nj4t-kc8j.json";
const MISSING_ADDRESS_SQL = "TRIM(UPPER(COALESCE(address, ''))) IN ('', 'CHICAGO, IL', ', CHICAGO, IL')";

interface CentroidRow {
  pin10?: string;
  year?: string;
  lat?: string;
  lon?: string;
}

export interface CentroidEnrichmentResult {
  eligible: number;
  matched: number;
  updated: number;
  missing: string[];
}

export interface CentroidEnrichmentOptions {
  batchSize: number;
}

function validateBatchSize(batchSize: number): void {
  if (!Number.isInteger(batchSize) || batchSize < 1 || batchSize > 100) {
    throw new Error("centroid batch size must be between 1 and 100");
  }
}

function chunks<T>(values: readonly T[], size: number): T[][] {
  const result: T[][] = [];
  for (let offset = 0; offset < values.length; offset += size) result.push(values.slice(offset, offset + size));
  return result;
}

function isValidLatitude(value: number): boolean {
  return Number.isFinite(value) && value >= -90 && value <= 90 && value !== 0;
}

function isValidLongitude(value: number): boolean {
  return Number.isFinite(value) && value >= -180 && value <= 180 && value !== 0;
}

export async function enrichMissingAddressCentroids(
  config: AppConfig,
  options: CentroidEnrichmentOptions,
  logger: Logger
): Promise<CentroidEnrichmentResult> {
  validateBatchSize(options.batchSize);
  const token = process.env.CHICAGO_DATA_PORTAL_TOKEN;
  if (token === undefined || token.length === 0) throw new Error("CHICAGO_DATA_PORTAL_TOKEN is required");

  const database = new LotsDatabase(config.databasePath);
  database.migrate();
  const db = database.connection;

  try {
    const candidates = db.prepare(`
      SELECT id FROM lots
      WHERE posted_bluesky = '0' AND ${MISSING_ADDRESS_SQL}
      ORDER BY id
    `).all().map((row) => String((row as { id: string }).id));
    const malformed = candidates.find((id) => !/^\d{10}$/.test(id));
    if (malformed !== undefined) throw new Error(`Invalid PIN10 in centroid candidate set: ${malformed}`);
    const candidateSet = new Set(candidates);
    const centroids = new Map<string, { lat: number; lon: number }>();

    for (const batch of chunks(candidates, options.batchSize)) {
      const pinList = batch.map((id) => `'${id}'`).join(",");
      const query = `SELECT pin10, year, lat, lon `
        + `WHERE pin10 IN (${pinList}) AND lat IS NOT NULL AND lon IS NOT NULL `
        + `ORDER BY pin10 ASC, year DESC LIMIT 50000`;
      const url = new URL(SOURCE_URL);
      url.searchParams.set("$query", query);
      const response = await fetch(url, {
        headers: { "X-App-Token": token },
        signal: AbortSignal.timeout(Math.max(config.httpTimeoutMs, 120_000))
      });
      if (!response.ok) throw new Error(`Cook County Parcel Universe API returned HTTP ${response.status}`);
      const rows = await response.json() as CentroidRow[];

      for (const row of rows) {
        const id = row.pin10 ?? "";
        if (!candidateSet.has(id) || centroids.has(id)) continue;
        const lat = Number(row.lat);
        const lon = Number(row.lon);
        if (!isValidLatitude(lat) || !isValidLongitude(lon)) continue;
        centroids.set(id, { lat, lon });
      }
      logger.info("centroid_page", { requested: batch.length, matched: batch.filter((id) => centroids.has(id)).length });
    }

    db.exec("BEGIN IMMEDIATE");
    let updated = 0;
    try {
      const update = db.prepare(`
        UPDATE lots SET lat = ?, lon = ?
        WHERE id = ? AND posted_bluesky = '0' AND ${MISSING_ADDRESS_SQL}
      `);
      for (const [id, centroid] of centroids) updated += Number(update.run(centroid.lat, centroid.lon, id).changes);
      db.exec("COMMIT");
    } catch (error) {
      db.exec("ROLLBACK");
      throw error;
    }

    return {
      eligible: candidates.length,
      matched: centroids.size,
      updated,
      missing: candidates.filter((id) => !centroids.has(id))
    };
  } finally {
    database.close();
  }
}
