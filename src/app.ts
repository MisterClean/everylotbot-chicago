import packageJson from "../package.json" with { type: "json" };
import type { AppConfig } from "./config.js";
import { LotsDatabase } from "./db.js";
import { composePost } from "./domain/address.js";
import type { Platform, Publisher } from "./domain/types.js";
import type { Logger } from "./logging.js";
import { errorFields } from "./logging.js";
import { BlueskyPublisher, isBlueskyRecordKey, newBlueskyRecordKey, PublishError } from "./platforms/bluesky.js";
import { TwitterPublisher } from "./platforms/twitter.js";
import { StreetViewClient } from "./services/street-view.js";

export interface RunOptions {
  dryRun: boolean;
  specificId?: string;
  requestedPlatforms?: Platform[];
}

export interface RunResult {
  outcome: "dry-run" | "posted" | "no-lot";
  lotId?: string;
  platforms?: Platform[];
}

function createPublisher(platform: Platform, config: AppConfig): Publisher {
  if (platform === "bluesky") {
    if (config.bluesky === undefined) throw new Error("Bluesky credentials are unavailable");
    return new BlueskyPublisher(config.bluesky);
  }
  if (config.twitter === undefined) throw new Error("Twitter credentials are unavailable");
  return new TwitterPublisher(config.twitter);
}

function effectivePlatforms(config: AppConfig, requested?: Platform[]): Platform[] {
  if (requested === undefined) return [...config.enabledPlatforms];
  for (const platform of requested) {
    if (!config.enabledPlatforms.includes(platform)) throw new Error(`${platform} is not enabled`);
  }
  return requested;
}

export async function runOnce(config: AppConfig, logger: Logger, options: RunOptions): Promise<RunResult> {
  const platforms = effectivePlatforms(config, options.requestedPlatforms);

  if (options.dryRun) {
    const database = new LotsDatabase(config.databasePath, { readOnly: true });
    try {
      const selection = database.selectNext(platforms, options.specificId, config.platformStarts);
      if (selection === null) return { outcome: "no-lot" };
      const post = composePost(selection.lot, config.printFormat);
      logger.info("dry_run", { lotId: selection.lot.id, platforms: selection.platforms, text: post.text, alt: post.alt });
      return { outcome: "dry-run", lotId: selection.lot.id, platforms: selection.platforms };
    } finally {
      database.close();
    }
  }

  if (config.googleApiKey === undefined) throw new Error("GOOGLE_API_KEY is required");
  const database = new LotsDatabase(config.databasePath);
  database.migrate();
  if (config.twitter?.startPin10 !== undefined && database.getHighWater("twitter") === null) {
    database.setPlatformStart("twitter", config.twitter.startPin10);
  }
  const leaseOwner = database.acquireLease("post-next", config.leaseSeconds);
  if (leaseOwner === null) {
    database.close();
    throw new Error("Another post-next invocation holds the database lease");
  }
  const runId = database.startRun(packageJson.version);

  try {
    const selection = database.selectNext(platforms, options.specificId, config.platformStarts);
    if (selection === null) {
      database.finishRun(runId, "no-lot");
      return { outcome: "no-lot" };
    }
    database.setRunLot(runId, selection.lot.id);
    const post = composePost(selection.lot, config.printFormat);
    const image = await new StreetViewClient(config).fetchImage(selection.lot);
    const errors: unknown[] = [];

    for (const platform of selection.platforms) {
      const previousDelivery = database.getDelivery(selection.lot.id, platform);
      const previousState = previousDelivery?.state ?? null;
      if ((previousState === "unknown" || previousState === "publishing") && platform !== "bluesky") {
        errors.push(new Error(`Refusing to retry uncertain ${platform} delivery for ${selection.lot.id}`));
        continue;
      }
      const previousKey = previousDelivery?.deterministicKey ?? null;
      const key = platform === "bluesky"
        ? (isBlueskyRecordKey(previousKey) ? previousKey : newBlueskyRecordKey())
        : undefined;
      database.beginDelivery(selection.lot.id, platform, key);
      try {
        const result = await createPublisher(platform, config).publish(selection.lot, post, image, key);
        database.confirmDelivery(selection.lot.id, platform, result.ref);
        logger.info("post_confirmed", { runId, lotId: selection.lot.id, platform, postRef: result.ref });
      } catch (error) {
        const uncertain = error instanceof PublishError && error.uncertain;
        database.failDelivery(selection.lot.id, platform, error, uncertain);
        logger.error("post_failed", { runId, lotId: selection.lot.id, platform, uncertain, ...errorFields(error) });
        errors.push(error);
      }
    }

    if (errors.length > 0) {
      database.finishRun(runId, "failed", "PLATFORM_FAILURE");
      throw new AggregateError(errors, `Posting failed for lot ${selection.lot.id}`);
    }
    database.finishRun(runId, "posted");
    return { outcome: "posted", lotId: selection.lot.id, platforms: selection.platforms };
  } catch (error) {
    try { database.finishRun(runId, "failed", "RUN_FAILURE"); } catch { /* retain original error */ }
    throw error;
  } finally {
    database.releaseLease("post-next", leaseOwner);
    database.close();
  }
}
