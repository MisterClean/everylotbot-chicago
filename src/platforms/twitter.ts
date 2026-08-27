import { createHmac, randomBytes } from "node:crypto";
import type { AppConfig } from "../config.js";
import type { ComposedPost, Lot, Publisher, PublishResult } from "../domain/types.js";
import { PublishError } from "./bluesky.js";

export class TwitterPublisher implements Publisher {
  readonly platform = "twitter" as const;
  constructor(private readonly config: NonNullable<AppConfig["twitter"]>) {}

  private authorization(method: string, url: string): string {
    const oauth: Record<string, string> = {
      oauth_consumer_key: this.config.appKey,
      oauth_nonce: randomBytes(18).toString("hex"),
      oauth_signature_method: "HMAC-SHA1",
      oauth_timestamp: String(Math.floor(Date.now() / 1000)),
      oauth_token: this.config.accessToken,
      oauth_version: "1.0"
    };
    const encode = (value: string): string => encodeURIComponent(value).replace(/[!'()*]/g, (character) => `%${character.charCodeAt(0).toString(16).toUpperCase()}`);
    const parameters = Object.entries(oauth).sort(([left], [right]) => left.localeCompare(right))
      .map(([key, value]) => `${encode(key)}=${encode(value)}`).join("&");
    const base = `${method}&${encode(url)}&${encode(parameters)}`;
    const signingKey = `${encode(this.config.appSecret)}&${encode(this.config.accessSecret)}`;
    oauth.oauth_signature = createHmac("sha1", signingKey).update(base).digest("base64");
    return `OAuth ${Object.entries(oauth).sort(([left], [right]) => left.localeCompare(right))
      .map(([key, value]) => `${encode(key)}="${encode(value)}"`).join(", ")}`;
  }

  async publish(_lot: Lot, post: ComposedPost, image: Uint8Array): Promise<PublishResult> {
    const uploadUrl = "https://upload.twitter.com/1.1/media/upload.json";
    const form = new FormData();
    const imageCopy = new Uint8Array(image.byteLength);
    imageCopy.set(image);
    form.set("media", new Blob([imageCopy.buffer], { type: "image/jpeg" }), "image.jpg");
    const uploadResponse = await fetch(uploadUrl, {
      method: "POST",
      headers: { Authorization: this.authorization("POST", uploadUrl) },
      body: form
    });
    if (!uploadResponse.ok) throw new PublishError(`Twitter media upload returned HTTP ${uploadResponse.status}`, false);
    const upload = await uploadResponse.json() as { media_id_string?: string };
    if (upload.media_id_string === undefined) throw new PublishError("Twitter media upload response had no media ID", false);
    const postUrl = "https://api.x.com/2/tweets";
    try {
      const response = await fetch(postUrl, {
        method: "POST",
        headers: { Authorization: this.authorization("POST", postUrl), "Content-Type": "application/json" },
        body: JSON.stringify({ text: post.text, media: { media_ids: [upload.media_id_string] } })
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const result = await response.json() as { data?: { id?: string } };
      if (result.data?.id === undefined) throw new Error("response had no post ID");
      return { ref: result.data.id };
    } catch (error) {
      throw new PublishError("Twitter post write failed with an uncertain outcome", true, { cause: error });
    }
  }
}
