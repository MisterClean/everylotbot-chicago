export type Platform = "bluesky" | "twitter";

export interface Lot {
  id: string;
  address: string;
  lat: number;
  lon: number;
  posted_twitter: string;
  posted_bluesky: string;
}

export interface ComposedPost {
  text: string;
  alt: string;
  lat: number;
  lon: number;
}

export interface PublishResult {
  ref: string;
}

export interface Publisher {
  readonly platform: Platform;
  publish(lot: Lot, post: ComposedPost, image: Uint8Array, deliveryKey?: string): Promise<PublishResult>;
}
