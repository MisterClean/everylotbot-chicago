import { DatabaseSync } from "node:sqlite";
import { afterEach, expect, it, vi } from "vitest";
import { enrichMissingAddressCentroids } from "../src/centroids.js";
import type { AppConfig } from "../src/config.js";
import type { Logger } from "../src/logging.js";
import { createFixtureDatabase } from "./fixtures.js";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
});

it("enriches only unposted missing-address parcels from the latest historic centroid", async () => {
  const path = createFixtureDatabase();
  const setup = new DatabaseSync(path);
  setup.prepare("UPDATE lots SET address = 'CHICAGO, IL' WHERE id = ?").run("1431213021");
  setup.close();
  vi.stubEnv("CHICAGO_DATA_PORTAL_TOKEN", "test-token");
  vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(JSON.stringify([
    { pin10: "1431213021", year: "2024", lat: "41.91", lon: "-87.68" },
    { pin10: "1431213021", year: "2023", lat: "41.90", lon: "-87.67" }
  ]), { status: 200, headers: { "content-type": "application/json" } }));
  const config: AppConfig = {
    databasePath: path,
    enabledPlatforms: ["bluesky"],
    platformStarts: {},
    printFormat: "{address}",
    streetviewPitch: 11.55,
    streetviewZoom: 0.9,
    streetviewRadiusMeters: 500,
    httpTimeoutMs: 1000,
    leaseSeconds: 600
  };
  const logger: Logger = { info: vi.fn(), error: vi.fn(), debug: vi.fn() };

  const result = await enrichMissingAddressCentroids(config, { batchSize: 75 }, logger);
  const db = new DatabaseSync(path, { readOnly: true });
  const row = db.prepare("SELECT lat, lon FROM lots WHERE id = ?").get("1431213021") as { lat: number; lon: number };
  db.close();

  expect(result).toEqual({ eligible: 1, matched: 1, updated: 1, missing: [] });
  expect(row).toEqual({ lat: 41.91, lon: -87.68 });
});
