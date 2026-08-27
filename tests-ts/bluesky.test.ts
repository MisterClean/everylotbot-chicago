import { describe, expect, it } from "vitest";
import { blueskyRecordKey } from "../src/platforms/bluesky.js";

describe("Bluesky idempotency key", () => {
  it("is deterministic for a PIN10", () => {
    expect(blueskyRecordKey("1614209047")).toBe("everylot-1614209047");
  });

  it("rejects malformed identifiers", () => {
    expect(() => blueskyRecordKey("123")).toThrow("Invalid PIN10");
  });
});
