import { parseArgs } from "node:util";
import type { Platform } from "../domain/types.js";

export interface CommonArguments {
  database?: string;
  verbose: boolean;
}

export function parsePlatforms(value: string | undefined): Platform[] | undefined {
  if (value === undefined || value === "all") return undefined;
  if (value === "bluesky" || value === "twitter") return [value];
  throw new Error("--platform must be bluesky, twitter, or all");
}

export function parsePostArguments(): CommonArguments & {
  dryRun: boolean;
  id?: string;
  platforms?: Platform[];
} {
  const { values } = parseArgs({
    options: {
      database: { type: "string" },
      id: { type: "string" },
      platform: { type: "string" },
      "dry-run": { type: "boolean", default: false },
      verbose: { type: "boolean", short: "v", default: false }
    },
    strict: true
  });
  if (values.id !== undefined && !/^\d{10}$/.test(values.id)) throw new Error("--id must be a 10-digit PIN10");
  const result: ReturnType<typeof parsePostArguments> = {
    dryRun: values["dry-run"],
    verbose: values.verbose
  };
  if (values.database !== undefined) result.database = values.database;
  if (values.id !== undefined) result.id = values.id;
  const platforms = parsePlatforms(values.platform);
  if (platforms !== undefined) result.platforms = platforms;
  return result;
}

export function parseAuditArguments(): CommonArguments {
  const { values } = parseArgs({
    options: {
      database: { type: "string" },
      verbose: { type: "boolean", short: "v", default: false }
    },
    strict: true
  });
  const result: CommonArguments = { verbose: values.verbose };
  if (values.database !== undefined) result.database = values.database;
  return result;
}
