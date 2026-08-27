import type { ComposedPost, Lot } from "./types.js";

const DIRECTIONS: Readonly<Record<string, string>> = {
  N: "North",
  S: "South",
  E: "East",
  W: "West"
};

const STREET_TYPES: Readonly<Record<string, string>> = {
  AVE: "Avenue",
  ST: "Street",
  BLVD: "Boulevard",
  RD: "Road",
  DR: "Drive",
  CT: "Court",
  PL: "Place",
  TER: "Terrace",
  LN: "Lane",
  WAY: "Way",
  CIR: "Circle",
  PKY: "Parkway",
  SQ: "Square"
};

const MISSING_ADDRESSES = new Set(["", "CHICAGO, IL", ", CHICAGO, IL"]);

export function hasUsableAddress(address: string): boolean {
  return !MISSING_ADDRESSES.has(address.trim().toUpperCase());
}

export function hasUsableCoordinates(lot: Pick<Lot, "lat" | "lon">): boolean {
  return Number.isFinite(lot.lat) && Number.isFinite(lot.lon)
    && lot.lat >= -90 && lot.lat <= 90
    && lot.lon >= -180 && lot.lon <= 180
    && lot.lat !== 0 && lot.lon !== 0;
}

export function sanitizeAddress(address: string): string {
  const parts = address.trim().split(",", 1)[0]?.split(/\s+/).filter(Boolean) ?? [];
  const result: string[] = [];

  for (const [index, rawPart] of parts.entries()) {
    const part = rawPart.trim();
    if (index === 0) {
      result.push(part);
    } else if (DIRECTIONS[part] !== undefined) {
      result.push(DIRECTIONS[part]);
    } else if (STREET_TYPES[part] !== undefined) {
      result.push(STREET_TYPES[part]);
      break;
    } else {
      result.push(part.charAt(0).toUpperCase() + part.slice(1).toLowerCase());
    }
  }

  return result.join(" ");
}

export function formatPost(template: string, lot: Lot): string {
  const values: Readonly<Record<string, string>> = {
    id: lot.id,
    address: sanitizeAddress(lot.address),
    lat: String(lot.lat),
    lon: String(lot.lon)
  };

  return template.replace(/\{([^{}]+)\}/g, (_match, key: string) => {
    const value = values[key];
    if (value === undefined) {
      throw new Error(`Unknown PRINT_FORMAT field: ${key}`);
    }
    return value;
  });
}

export function composePost(lot: Lot, printFormat: string): ComposedPost {
  if (!hasUsableAddress(lot.address)) {
    if (!hasUsableCoordinates(lot)) throw new Error(`Lot ${lot.id} has neither a usable address nor coordinates`);
    return {
      text: lot.id,
      alt: `Google Street View near Cook County parcel PIN10 ${lot.id}. This parcel does not have a common address.`,
      lat: lot.lat,
      lon: lot.lon
    };
  }
  const cleanAddress = sanitizeAddress(lot.address);
  return {
    text: formatPost(printFormat, lot),
    alt: `Google Streetview of the property with PIN10 ${lot.id}: ${cleanAddress}`,
    lat: lot.lat,
    lon: lot.lon
  };
}
