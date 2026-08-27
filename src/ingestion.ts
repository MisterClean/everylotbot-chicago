import type { AppConfig } from "./config.js";
import { LotsDatabase } from "./db.js";
import type { Logger } from "./logging.js";

const SOURCE_URL = "https://datacatalog.cookcountyil.gov/resource/3723-97qp.csv";

interface SourceRow {
  pin: string;
  pin10: string;
  prop_address_full: string;
  prop_address_city_name: string;
  prop_address_state: string;
  prop_address_zipcode_1: string;
}

export async function* parseCsv(body: ReadableStream<Uint8Array>): AsyncGenerator<Record<string, string>> {
  const decoder = new TextDecoder();
  const reader = body.getReader();
  let headers: string[] | undefined;
  let row: string[] = [];
  let field = "";
  let inQuotes = false;
  let quotePending = false;

  const consumeRow = (): Record<string, string> | null => {
    row.push(field);
    field = "";
    if (headers === undefined) {
      headers = row.map((value, index) => index === 0 ? value.replace(/^\uFEFF/, "") : value);
      row = [];
      return null;
    }
    const record: Record<string, string> = {};
    for (const [index, header] of headers.entries()) record[header] = row[index] ?? "";
    row = [];
    return record;
  };

  const consume = function* (text: string): Generator<Record<string, string>> {
    for (const character of text) {
      if (quotePending) {
        if (character === '"') {
          field += '"';
          quotePending = false;
          continue;
        }
        inQuotes = false;
        quotePending = false;
      }
      if (inQuotes) {
        if (character === '"') quotePending = true;
        else field += character;
      } else if (character === '"' && field.length === 0) {
        inQuotes = true;
      } else if (character === ",") {
        row.push(field);
        field = "";
      } else if (character === "\n") {
        const record = consumeRow();
        if (record !== null) yield record;
      } else if (character !== "\r") {
        field += character;
      }
    }
  };

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    yield* consume(decoder.decode(value, { stream: true }));
  }
  yield* consume(decoder.decode());
  if (field.length > 0 || row.length > 0) {
    const record = consumeRow();
    if (record !== null) yield record;
  }
}

export interface IngestOptions {
  year: string;
  city: string;
  batchSize: number;
}

function validateOptions(options: IngestOptions): void {
  if (!/^\d{4}$/.test(options.year)) throw new Error("year must be four digits");
  if (!/^[A-Za-z .-]+$/.test(options.city)) throw new Error("city contains unsupported characters");
  if (!Number.isInteger(options.batchSize) || options.batchSize < 1 || options.batchSize > 50_000) {
    throw new Error("batch size must be between 1 and 50000");
  }
}

export async function ingest(config: AppConfig, options: IngestOptions, logger: Logger): Promise<{ fetched: number; staged: number }> {
  validateOptions(options);
  const token = process.env.CHICAGO_DATA_PORTAL_TOKEN;
  if (token === undefined || token.length === 0) throw new Error("CHICAGO_DATA_PORTAL_TOKEN is required");
  const database = new LotsDatabase(config.databasePath);
  database.migrate();
  const db = database.connection;
  db.exec(`
    DROP TABLE IF EXISTS temp.import_lots;
    CREATE TEMP TABLE import_lots (
      id TEXT PRIMARY KEY,
      pin14 TEXT NOT NULL,
      address TEXT NOT NULL
    );
  `);
  const insert = db.prepare(`
    INSERT INTO import_lots(id, pin14, address) VALUES (?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      pin14 = CASE WHEN excluded.address != '' THEN excluded.pin14 ELSE import_lots.pin14 END,
      address = CASE WHEN excluded.address != '' THEN excluded.address ELSE import_lots.address END
  `);
  let offset = 0;
  let fetched = 0;

  try {
    while (true) {
      const query = `SELECT pin, pin10, year, prop_address_full, prop_address_city_name, prop_address_state, prop_address_zipcode_1 ` +
        `WHERE year IN ('${options.year}') AND caseless_one_of(prop_address_city_name, '${options.city}', '${options.city.toLowerCase()}') ` +
        `ORDER BY pin ASC LIMIT ${options.batchSize} OFFSET ${offset}`;
      const url = new URL(SOURCE_URL);
      url.searchParams.set("$query", query);
      const response = await fetch(url, {
        headers: { "X-App-Token": token },
        signal: AbortSignal.timeout(Math.max(config.httpTimeoutMs, 120_000))
      });
      if (!response.ok) throw new Error(`Cook County API returned HTTP ${response.status}`);
      if (response.body === null) throw new Error("Cook County API returned no body");
      let batchCount = 0;
      let rawCount = 0;
      db.exec("BEGIN");
      try {
        for await (const raw of parseCsv(response.body)) {
          rawCount += 1;
          const row = raw as unknown as SourceRow;
          if (!/^\d{10}$/.test(row.pin10) || !/^\d{14}$/.test(row.pin)) continue;
          const street = row.prop_address_full.trim();
          const address = street.length === 0
            ? ""
            : `${street}, ${row.prop_address_city_name.trim()}, ${row.prop_address_state.trim()} ${row.prop_address_zipcode_1.trim()}`.replace(/,?\s+$/, "");
          insert.run(row.pin10, row.pin, address);
          batchCount += 1;
        }
        db.exec("COMMIT");
      } catch (error) {
        db.exec("ROLLBACK");
        throw error;
      }
      fetched += batchCount;
      logger.info("ingest_page", { offset, fetched: batchCount });
      if (rawCount < options.batchSize) break;
      offset += options.batchSize;
    }

    const stagedRow = db.prepare("SELECT COUNT(*) count FROM import_lots").get() as { count: number };
    if (stagedRow.count === 0) throw new Error("Import produced zero valid lots; refusing to modify the live table");
    db.exec("BEGIN IMMEDIATE");
    try {
      db.exec(`
        INSERT INTO lots(id, address, lat, lon, posted_twitter, posted_bluesky)
        SELECT id, address, 0.0, 0.0, '0', '0' FROM import_lots WHERE true
        ON CONFLICT(id) DO UPDATE SET
          address = CASE WHEN TRIM(excluded.address) != '' THEN excluded.address ELSE lots.address END;
        COMMIT;
      `);
    } catch (error) {
      db.exec("ROLLBACK");
      throw error;
    }
    return { fetched, staged: Number(stagedRow.count) };
  } finally {
    database.close();
  }
}
