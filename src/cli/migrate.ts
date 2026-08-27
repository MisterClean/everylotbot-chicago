#!/usr/bin/env node
import { readConfig } from "../config.js";
import { LotsDatabase } from "../db.js";
import { errorFields } from "../logging.js";
import { parseAuditArguments } from "./arguments.js";

function main(): void {
  const args = parseAuditArguments();
  const config = readConfig({ requirePostingSecrets: false });
  if (args.database !== undefined) config.databasePath = args.database;
  const database = new LotsDatabase(config.databasePath);
  try {
    database.migrate();
    process.stdout.write(`${JSON.stringify({ event: "migration_complete" })}\n`);
  } finally {
    database.close();
  }
}

try {
  main();
} catch (error) {
  process.stderr.write(`${JSON.stringify({ event: "migration_failed", ...errorFields(error) })}\n`);
  process.exitCode = 1;
}
