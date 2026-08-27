import { describe, expect, it } from "vitest";
import { isBlueskyRecordKey, newBlueskyRecordKey } from "../src/platforms/bluesky.js";

describe("Bluesky idempotency key", () => {
  it("generates a valid feed-post TID", () => {
    const key = newBlueskyRecordKey();
    expect(key).toHaveLength(13);
    expect(isBlueskyRecordKey(key)).toBe(true);
  });

  it("rejects the legacy human-readable key", () => {
    expect(isBlueskyRecordKey("everylot-1614209047")).toBe(false);
    expect(isBlueskyRecordKey(null)).toBe(false);
  });
});
