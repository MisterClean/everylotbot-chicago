#!/usr/bin/env node
import { parseArgs } from "node:util";
import { readConfig } from "../config.js";
import { ingest } from "../ingestion.js";
import { createLogger, errorFields } from "../logging.js";

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: {
      database: { type: "string" },
      year: { type: "string", default: "2023" },
      city: { type: "string", default: "CHICAGO" },
      "batch-size": { type: "string", default: "50000" },
      verbose: { type: "boolean", short: "v", default: false }
    },
    strict: true
  });
  const config = readConfig({ requirePostingSecrets: false });
  if (values.database !== undefined) config.databasePath = values.database;
  const logger = createLogger(values.verbose);
  const result = await ingest(config, { year: values.year, city: values.city, batchSize: Number(values["batch-size"]) }, logger);
  logger.info("ingest_complete", result);
}

main().catch((error: unknown) => {
  createLogger().error("ingest_failed", errorFields(error));
  process.exitCode = 1;
});
