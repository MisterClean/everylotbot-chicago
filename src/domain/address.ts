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
  const cleanAddress = sanitizeAddress(lot.address);
  return {
    text: formatPost(printFormat, lot),
    alt: `Google Streetview of the property with PIN10 ${lot.id}: ${cleanAddress}`,
    lat: lot.lat,
    lon: lot.lon
  };
}
