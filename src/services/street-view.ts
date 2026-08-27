import type { AppConfig } from "../config.js";
import { hasUsableAddress, hasUsableCoordinates } from "../domain/address.js";
import type { Lot } from "../domain/types.js";

const STREET_VIEW_URL = "https://maps.googleapis.com/maps/api/streetview";

export class StreetViewClient {
  constructor(private readonly config: Pick<AppConfig, "googleApiKey" | "streetviewPitch" | "streetviewZoom" | "streetviewRadiusMeters" | "httpTimeoutMs">) {}

  buildUrl(lot: Lot): URL {
    if (this.config.googleApiKey === undefined) throw new Error("GOOGLE_API_KEY is required");
    const url = new URL(STREET_VIEW_URL);
    if (hasUsableAddress(lot.address)) {
      // This deliberately preserves the production Python request for addressed parcels.
      url.searchParams.set("location", `${lot.address}, CHICAGO, IL`);
    } else {
      if (!hasUsableCoordinates(lot)) throw new Error(`Lot ${lot.id} has neither a usable address nor coordinates`);
      url.searchParams.set("location", `${lot.lat},${lot.lon}`);
      url.searchParams.set("radius", String(this.config.streetviewRadiusMeters));
      url.searchParams.set("source", "outdoor");
    }
    url.searchParams.set("key", this.config.googleApiKey);
    url.searchParams.set("size", "1000x1000");
    url.searchParams.set("fov", "65");
    url.searchParams.set("pitch", String(this.config.streetviewPitch));
    url.searchParams.set("zoom", String(this.config.streetviewZoom));
    url.searchParams.set("return_error_code", "true");
    return url;
  }

  async fetchImage(lot: Lot): Promise<Uint8Array> {
    const url = this.buildUrl(lot);
    let lastError: unknown;
    for (let attempt = 1; attempt <= 3; attempt += 1) {
      try {
        const response = await fetch(url, { signal: AbortSignal.timeout(this.config.httpTimeoutMs) });
        if (!response.ok) throw new Error(`Street View returned HTTP ${response.status}`);
        const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
        if (!contentType.startsWith("image/")) throw new Error(`Street View returned unexpected content type: ${contentType || "missing"}`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (bytes.byteLength === 0) throw new Error("Street View returned an empty image");
        return bytes;
      } catch (error) {
        lastError = error;
        if (attempt < 3) await new Promise((resolve) => setTimeout(resolve, 250 * 2 ** (attempt - 1)));
      }
    }
    throw lastError;
  }
}
