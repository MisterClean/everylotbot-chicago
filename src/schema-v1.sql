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
