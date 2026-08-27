import { afterEach, describe, expect, it, vi } from "vitest";
import { readConfig } from "../src/config.js";

afterEach(() => vi.unstubAllEnvs());

describe("configuration safety", () => {
  it("accepts blank optional Twitter values while Twitter is disabled", () => {
    vi.stubEnv("ENABLE_BLUESKY", "true");
    vi.stubEnv("ENABLE_TWITTER", "false");
    vi.stubEnv("TWITTER_START_PIN10", "");
    const config = readConfig({ requirePostingSecrets: false });
    expect(config.enabledPlatforms).toEqual(["bluesky"]);
    expect(config.platformStarts).toEqual({});
    expect(config.streetviewRadiusMeters).toBe(500);
  });

  it("refuses Twitter without an explicit starting PIN10", () => {
    vi.stubEnv("ENABLE_BLUESKY", "false");
    vi.stubEnv("ENABLE_TWITTER", "true");
    vi.stubEnv("TWITTER_START_PIN10", "");
    expect(() => readConfig({ requirePostingSecrets: false })).toThrow("TWITTER_START_PIN10 is required");
  });

  it("normalizes dotenv-style quotes preserved by Docker env files", () => {
    vi.stubEnv("ENABLE_BLUESKY", "true");
    vi.stubEnv("ENABLE_TWITTER", "false");
    vi.stubEnv("PRINT_FORMAT", '"{address}"');
    expect(readConfig({ requirePostingSecrets: false }).printFormat).toBe("{address}");
  });
});
