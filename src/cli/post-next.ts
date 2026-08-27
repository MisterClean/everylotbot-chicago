#!/usr/bin/env node
import { runOnce } from "../app.js";
import { readConfig } from "../config.js";
import { createLogger, errorFields } from "../logging.js";
import { parsePostArguments } from "./arguments.js";

async function main(): Promise<void> {
  const args = parsePostArguments();
  const config = readConfig({ requirePostingSecrets: !args.dryRun });
  if (args.database !== undefined) config.databasePath = args.database;
  const logger = createLogger(args.verbose);
  const options: Parameters<typeof runOnce>[2] = { dryRun: args.dryRun };
  if (args.id !== undefined) options.specificId = args.id;
  if (args.platforms !== undefined) options.requestedPlatforms = args.platforms;
  const result = await runOnce(config, logger, options);
  logger.info("run_complete", { ...result });
}

main().catch((error: unknown) => {
  createLogger().error("run_failed", errorFields(error));
  process.exitCode = 1;
});
