#!/usr/bin/env node
import { parseArgs } from "node:util";
import { enrichMissingAddressCentroids } from "../centroids.js";
import { readConfig } from "../config.js";
import { createLogger, errorFields } from "../logging.js";

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: {
      database: { type: "string" },
      "batch-size": { type: "string", default: "75" },
      verbose: { type: "boolean", short: "v", default: false }
    },
    strict: true
  });
  const config = readConfig({ requirePostingSecrets: false });
  if (values.database !== undefined) config.databasePath = values.database;
  const logger = createLogger(values.verbose);
  const result = await enrichMissingAddressCentroids(config, { batchSize: Number(values["batch-size"]) }, logger);
  logger.info("centroid_enrichment_complete", { ...result });
}

main().catch((error: unknown) => {
  createLogger().error("centroid_enrichment_failed", errorFields(error));
  process.exitCode = 1;
});
