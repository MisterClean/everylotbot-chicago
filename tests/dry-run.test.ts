import { DatabaseSync } from "node:sqlite";
import { expect, it, vi } from "vitest";
import { runOnce } from "../src/app.js";
import type { AppConfig } from "../src/config.js";
import type { Logger } from "../src/logging.js";
import { createFixtureDatabase } from "./fixtures.js";

it("dry run is read-only and does not call fetch", async () => {
  const path = createFixtureDatabase();
  const before = new DatabaseSync(path, { readOnly: true }).prepare("SELECT COUNT(*) count FROM sqlite_master").get() as { count: number };
  const fetchSpy = vi.spyOn(globalThis, "fetch");
  const logger: Logger = { info: vi.fn(), error: vi.fn(), debug: vi.fn() };
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
  const result = await runOnce(config, logger, { dryRun: true });
  const raw = new DatabaseSync(path, { readOnly: true });
  const after = raw.prepare("SELECT COUNT(*) count FROM sqlite_master").get() as { count: number };
  raw.close();

  expect(result).toMatchObject({ outcome: "dry-run", lotId: "1431213021" });
  expect(after.count).toBe(before.count);
  expect(fetchSpy).not.toHaveBeenCalled();
});
