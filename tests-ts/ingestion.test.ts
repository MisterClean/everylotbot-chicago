import { DatabaseSync } from "node:sqlite";
import { afterEach, expect, it, vi } from "vitest";
import { ingest, parseCsv } from "../src/ingestion.js";
import type { AppConfig } from "../src/config.js";
import type { Logger } from "../src/logging.js";
import { createFixtureDatabase } from "./fixtures.js";

afterEach(() => vi.restoreAllMocks());

it("parses quoted commas and quotes across streamed chunks", async () => {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encoder.encode('pin,pin10,prop_address_full\r\n"123456789012'));
      controller.enqueue(encoder.encode('34",1234567890,"12 ""A"", MAIN ST"\r\n'));
      controller.close();
    }
  });
  const rows: Array<Record<string, string>> = [];
  for await (const row of parseCsv(body)) rows.push(row);
  expect(rows).toEqual([{ pin: "12345678901234", pin10: "1234567890", prop_address_full: '12 "A", MAIN ST' }]);
});

it("updates source addresses without erasing posting progress", async () => {
  const path = createFixtureDatabase();
  process.env.CHICAGO_DATA_PORTAL_TOKEN = "test-token";
  vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(
    "pin,pin10,prop_address_full,prop_address_city_name,prop_address_state,prop_address_zipcode_1\n" +
    "14312130180000,1431213018,2025 N DAMEN AVE,CHICAGO,IL,60647\n",
    { status: 200, headers: { "content-type": "text/csv" } }
  ));
  const config: AppConfig = {
    databasePath: path,
    enabledPlatforms: ["bluesky"],
    platformStarts: {},
    printFormat: "{address}",
    streetviewPitch: 11.55,
    streetviewZoom: 0.9,
    httpTimeoutMs: 1000,
    leaseSeconds: 600
  };
  const logger: Logger = { info: vi.fn(), error: vi.fn(), debug: vi.fn() };
  await ingest(config, { year: "2023", city: "CHICAGO", batchSize: 2 }, logger);

  const db = new DatabaseSync(path, { readOnly: true });
  const row = db.prepare("SELECT address, posted_bluesky FROM lots WHERE id = ?").get("1431213018") as { address: string; posted_bluesky: string };
  db.close();
  expect(row.address).toBe("2025 N DAMEN AVE, CHICAGO, IL 60647");
  expect(row.posted_bluesky).toContain("/post/old1");
});
