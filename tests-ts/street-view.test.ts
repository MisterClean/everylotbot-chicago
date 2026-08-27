import { expect, it } from "vitest";
import { StreetViewClient } from "../src/services/street-view.js";
import type { Lot } from "../src/domain/types.js";

const lot: Lot = {
  id: "1614209047",
  address: "3412 W JACKSON BLVD, CHICAGO, IL 60624",
  lat: 0,
  lon: 0,
  posted_twitter: "0",
  posted_bluesky: "0"
};

it("preserves production Street View parameters without exposing them in logs", () => {
  const url = new StreetViewClient({ googleApiKey: "secret", streetviewPitch: 11.55, streetviewZoom: 0.9, httpTimeoutMs: 1000 }).buildUrl(lot);
  expect(url.searchParams.get("location")).toBe("3412 W JACKSON BLVD, CHICAGO, IL 60624, CHICAGO, IL");
  expect(url.searchParams.get("size")).toBe("1000x1000");
  expect(url.searchParams.get("fov")).toBe("65");
  expect(url.searchParams.get("pitch")).toBe("11.55");
  expect(url.searchParams.get("zoom")).toBe("0.9");
});
