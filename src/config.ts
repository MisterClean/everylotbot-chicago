import { config as loadDotenv } from "dotenv";
import { z } from "zod";
import type { Platform } from "./domain/types.js";

const booleanString = z.string().transform((value, context) => {
  const normalized = value.toLowerCase();
  if (normalized !== "true" && normalized !== "false") {
    context.addIssue({ code: "custom", message: "must be true or false" });
    return z.NEVER;
  }
  return normalized === "true";
});

const optionalString = z.preprocess(
  (value) => value === "" ? undefined : value,
  z.string().min(1).optional()
);

const optionalPin10 = z.preprocess(
  (value) => value === "" ? undefined : value,
  z.string().regex(/^\d{10}$/).optional()
);

const printFormat = z.string().transform((value) => {
  const quote = value.at(0);
  if ((quote === "\"" || quote === "'") && value.at(-1) === quote) {
    return value.slice(1, -1);
  }
  return value;
}).pipe(z.string().min(1));

const envSchema = z.object({
  DATABASE_PATH: z.string().min(1).default("cook_county_lots.db"),
  GOOGLE_API_KEY: optionalString,
  BLUESKY_IDENTIFIER: optionalString,
  BLUESKY_PASSWORD: optionalString,
  BLUESKY_SERVICE: z.url().default("https://bsky.social"),
  BLUESKY_SESSION_PATH: z.string().min(1).default("var/bluesky-session.json"),
  TWITTER_CONSUMER_KEY: optionalString,
  TWITTER_CONSUMER_SECRET: optionalString,
  TWITTER_ACCESS_TOKEN: optionalString,
  TWITTER_ACCESS_TOKEN_SECRET: optionalString,
  TWITTER_START_PIN10: optionalPin10,
  ENABLE_BLUESKY: booleanString.default(true),
  ENABLE_TWITTER: booleanString.default(false),
  PRINT_FORMAT: printFormat.default("{address}"),
  STREETVIEW_PITCH: z.coerce.number().default(11.55),
  STREETVIEW_ZOOM: z.coerce.number().default(0.9),
  STREETVIEW_RADIUS_METERS: z.coerce.number().int().min(1).max(1000).default(500),
  HTTP_TIMEOUT_MS: z.coerce.number().int().positive().default(30_000),
  LEASE_SECONDS: z.coerce.number().int().min(60).max(840).default(840)
});

export interface AppConfig {
  databasePath: string;
  googleApiKey?: string;
  bluesky?: {
    identifier: string;
    password: string;
    service: string;
    sessionPath: string;
  };
  twitter?: {
    appKey: string;
    appSecret: string;
    accessToken: string;
    accessSecret: string;
    startPin10?: string;
  };
  enabledPlatforms: Platform[];
  platformStarts: Partial<Record<Platform, string>>;
  printFormat: string;
  streetviewPitch: number;
  streetviewZoom: number;
  streetviewRadiusMeters: number;
  httpTimeoutMs: number;
  leaseSeconds: number;
}

export function readConfig(options: { requirePostingSecrets: boolean } = { requirePostingSecrets: true }): AppConfig {
  loadDotenv({ quiet: true });
  const env = envSchema.parse(process.env);
  const enabledPlatforms: Platform[] = [];
  let bluesky: AppConfig["bluesky"];
  let twitter: AppConfig["twitter"];

  if (env.ENABLE_BLUESKY) {
    enabledPlatforms.push("bluesky");
    if (options.requirePostingSecrets) {
      if (env.BLUESKY_IDENTIFIER === undefined || env.BLUESKY_PASSWORD === undefined) {
        throw new Error("BLUESKY_IDENTIFIER and BLUESKY_PASSWORD are required");
      }
      bluesky = {
        identifier: env.BLUESKY_IDENTIFIER,
        password: env.BLUESKY_PASSWORD,
        service: env.BLUESKY_SERVICE,
        sessionPath: env.BLUESKY_SESSION_PATH
      };
    }
  }

  if (env.ENABLE_TWITTER) {
    enabledPlatforms.push("twitter");
    if (env.TWITTER_START_PIN10 === undefined) {
      throw new Error("TWITTER_START_PIN10 is required when enabling Twitter to prevent accidental historical backfill");
    }
    if (options.requirePostingSecrets) {
      const credentials = [env.TWITTER_CONSUMER_KEY, env.TWITTER_CONSUMER_SECRET, env.TWITTER_ACCESS_TOKEN, env.TWITTER_ACCESS_TOKEN_SECRET];
      if (credentials.some((value) => value === undefined)) {
        throw new Error("All Twitter credentials are required when Twitter is enabled");
      }
      twitter = {
        appKey: env.TWITTER_CONSUMER_KEY!,
        appSecret: env.TWITTER_CONSUMER_SECRET!,
        accessToken: env.TWITTER_ACCESS_TOKEN!,
        accessSecret: env.TWITTER_ACCESS_TOKEN_SECRET!,
        startPin10: env.TWITTER_START_PIN10
      };
    }
  }

  if (enabledPlatforms.length === 0) {
    throw new Error("At least one platform must be enabled");
  }

  const result: AppConfig = {
    databasePath: env.DATABASE_PATH,
    enabledPlatforms,
    platformStarts: env.TWITTER_START_PIN10 === undefined ? {} : { twitter: env.TWITTER_START_PIN10 },
    printFormat: env.PRINT_FORMAT,
    streetviewPitch: env.STREETVIEW_PITCH,
    streetviewZoom: env.STREETVIEW_ZOOM,
    streetviewRadiusMeters: env.STREETVIEW_RADIUS_METERS,
    httpTimeoutMs: env.HTTP_TIMEOUT_MS,
    leaseSeconds: env.LEASE_SECONDS
  };
  if (env.GOOGLE_API_KEY !== undefined) result.googleApiKey = env.GOOGLE_API_KEY;
  if (bluesky !== undefined) result.bluesky = bluesky;
  if (twitter !== undefined) result.twitter = twitter;
  return result;
}
