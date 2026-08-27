import { DatabaseSync } from "node:sqlite";
import { describe, expect, it } from "vitest";
import { LotsDatabase } from "../src/db.js";
import { createFixtureDatabase } from "./fixtures.js";

describe("production cursor compatibility", () => {
  it("never backfills pending lots below the high-water mark", () => {
    const path = createFixtureDatabase();
    const database = new LotsDatabase(path, { readOnly: true });
    expect(database.getHighWater("bluesky")).toBe("1431213020");
    expect(database.getNextForPlatform("bluesky")?.id).toBe("1431213021");
    const audit = database.audit(["bluesky"]);
    expect(audit.skippedBeforeBlueskyStart).toBe(2);
    expect(audit.gapsInBlueskyRun).toBe(0);
    database.close();
  });

  it("adds only backward-compatible tables and confirms atomically", () => {
    const path = createFixtureDatabase();
    const database = new LotsDatabase(path);
    database.migrate();
    database.beginDelivery("1431213021", "bluesky", "everylot-1431213021");
    database.confirmDelivery("1431213021", "bluesky", "https://bsky.app/profile/did:plc:test/post/new");
    expect(database.getHighWater("bluesky")).toBe("1431213021");
    expect(database.getDeliveryState("1431213021", "bluesky")).toBe("confirmed");
    database.close();

    const raw = new DatabaseSync(path, { readOnly: true });
    const row = raw.prepare("SELECT posted_bluesky FROM lots WHERE id = ?").get("1431213021") as { posted_bluesky: string };
    expect(row.posted_bluesky).toContain("/post/new");
    raw.close();
  });

  it("prevents a second lease until the first is released", () => {
    const database = new LotsDatabase(createFixtureDatabase());
    database.migrate();
    const owner = database.acquireLease("post-next", 600);
    expect(owner).not.toBeNull();
    expect(database.acquireLease("post-next", 600)).toBeNull();
    database.releaseLease("post-next", owner!);
    expect(database.acquireLease("post-next", 600)).not.toBeNull();
    database.close();
  });

  it("lets a lagging secondary platform catch up without reposting to Bluesky", () => {
    const database = new LotsDatabase(createFixtureDatabase());
    database.migrate();
    database.setPlatformStart("twitter", "1431213019");
    const selection = database.selectNext(["bluesky", "twitter"]);
    expect(selection?.lot.id).toBe("1431213020");
    expect(selection?.platforms).toEqual(["twitter"]);
    database.close();
  });
});
