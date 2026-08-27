import { describe, expect, it } from "vitest";
import { composePost, formatPost, hasUsableAddress, sanitizeAddress } from "../src/domain/address.js";
import type { Lot } from "../src/domain/types.js";

const lot: Lot = {
  id: "1431213020",
  address: "2017 N DAMEN AVE 1, CHICAGO, IL 60647",
  lat: 0,
  lon: 0,
  posted_twitter: "0",
  posted_bluesky: "0"
};

describe("production-compatible composition", () => {
  it("sanitizes and stops at the street suffix", () => {
    expect(sanitizeAddress(lot.address)).toBe("2017 North Damen Avenue");
  });

  it("preserves current text and alt formats", () => {
    expect(composePost(lot, "{address}")).toEqual({
      text: "2017 North Damen Avenue",
      alt: "Google Streetview of the property with PIN10 1431213020: 2017 North Damen Avenue",
      lat: 0,
      lon: 0
    });
  });

  it("rejects unknown format fields", () => {
    expect(() => formatPost("{city}", lot)).toThrow("Unknown PRINT_FORMAT field");
  });

  it("uses only PIN10 text for a centroid-backed parcel without an address", () => {
    const centroidLot = { ...lot, address: "CHICAGO, IL", lat: 41.9, lon: -87.7 };
    expect(hasUsableAddress(centroidLot.address)).toBe(false);
    expect(composePost(centroidLot, "{address}")).toEqual({
      text: "1431213020",
      alt: "Google Street View near Cook County parcel PIN10 1431213020. This parcel does not have a common address.",
      lat: 41.9,
      lon: -87.7
    });
  });

  it("refuses an unlocated parcel instead of publishing a false address", () => {
    expect(() => composePost({ ...lot, address: "CHICAGO, IL" }, "{address}"))
      .toThrow("neither a usable address nor coordinates");
  });
});
