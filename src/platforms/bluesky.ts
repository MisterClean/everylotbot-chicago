import { Agent, AppBskyFeedPost, CredentialSession, type AtpSessionData } from "@atproto/api";
import { TID } from "@atproto/common-web";
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import type { AppConfig } from "../config.js";
import type { ComposedPost, Lot, Publisher, PublishResult } from "../domain/types.js";

export class PublishError extends Error {
  constructor(message: string, readonly uncertain: boolean, options?: ErrorOptions) {
    super(message, options);
    this.name = "PublishError";
  }
}

export function newBlueskyRecordKey(): string {
  return TID.nextStr();
}

export function isBlueskyRecordKey(value: string | null): value is string {
  return value !== null && TID.is(value);
}

function publicUrl(uri: string): string {
  const parts = uri.split("/");
  const did = parts[2];
  const rkey = parts.at(-1);
  if (did === undefined || rkey === undefined || !uri.startsWith("at://")) return uri;
  return `https://bsky.app/profile/${did}/post/${rkey}`;
}

function isRecordNotFound(error: unknown): boolean {
  if (error instanceof Error && /RecordNotFound|Could not locate record|404/i.test(error.message)) return true;
  if (typeof error === "object" && error !== null) {
    const candidate = error as { status?: unknown; error?: unknown };
    return candidate.status === 404 || candidate.error === "RecordNotFound";
  }
  return false;
}

export class BlueskyPublisher implements Publisher {
  readonly platform = "bluesky" as const;
  private readonly agent: Agent;
  private readonly session: CredentialSession;

  constructor(private readonly config: NonNullable<AppConfig["bluesky"]>) {
    this.session = new CredentialSession(
      new URL(config.service),
      undefined,
      (_event, session) => {
        if (session !== undefined) this.persistSession(session);
      }
    );
    // CredentialSession is the documented Agent session manager. Its published
    // type currently conflicts with exactOptionalPropertyTypes on `did`.
    this.agent = new Agent(this.session as unknown as ConstructorParameters<typeof Agent>[0]);
  }

  private persistSession(session: AtpSessionData): void {
    mkdirSync(dirname(this.config.sessionPath), { recursive: true, mode: 0o700 });
    const temporary = `${this.config.sessionPath}.tmp`;
    writeFileSync(temporary, JSON.stringify(session), { encoding: "utf8", mode: 0o600 });
    renameSync(temporary, this.config.sessionPath);
  }

  private async authenticate(): Promise<void> {
    try {
      const stored = JSON.parse(readFileSync(this.config.sessionPath, "utf8")) as AtpSessionData;
      await this.session.resumeSession(stored);
      return;
    } catch {
      // A missing, expired, or corrupt session falls back to a fresh login.
    }
    await this.session.login({ identifier: this.config.identifier, password: this.config.password });
    if (this.session.session !== undefined) this.persistSession(this.session.session);
  }

  async publish(_lot: Lot, post: ComposedPost, image: Uint8Array, deliveryKey?: string): Promise<PublishResult> {
    await this.authenticate();
    const did = this.agent.did;
    if (did === undefined) throw new PublishError("Bluesky session did not contain a DID", false);
    if (deliveryKey === undefined || !isBlueskyRecordKey(deliveryKey)) {
      throw new PublishError("Bluesky delivery requires a valid TID record key", false);
    }
    const rkey = deliveryKey;
    const collection = "app.bsky.feed.post";

    try {
      const existing = await this.agent.com.atproto.repo.getRecord({ repo: did, collection, rkey });
      const value = existing.data.value as { text?: unknown };
      if (value.text !== post.text) throw new PublishError(`Existing Bluesky record ${rkey} has unexpected text`, false);
      return { ref: publicUrl(existing.data.uri) };
    } catch (error) {
      if (!isRecordNotFound(error)) throw error;
    }

    const upload = await this.agent.uploadBlob(image, { encoding: "image/jpeg" });
    const record = {
      $type: collection,
      text: post.text,
      createdAt: new Date().toISOString(),
      embed: {
        $type: "app.bsky.embed.images",
        images: [{ image: upload.data.blob, alt: post.alt }]
      }
    } satisfies AppBskyFeedPost.Record;

    const validation = AppBskyFeedPost.validateRecord(record);
    if (!validation.success) throw new PublishError("Generated Bluesky post did not pass schema validation", false);

    try {
      const result = await this.agent.com.atproto.repo.createRecord({ repo: did, collection, rkey, record, validate: true });
      return { ref: publicUrl(result.data.uri) };
    } catch (error) {
      throw new PublishError("Bluesky record write failed with an uncertain outcome", true, { cause: error });
    }
  }
}
