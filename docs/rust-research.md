# Rust rewrite research

Research snapshot: 18 September 2026, approximately 14:49–14:53 UTC. This report records the existing TypeScript implementation and a read-only inspection of the production Lightsail host. It does not describe an executed migration or deployment.

## Findings that govern the rewrite

1. The production bot is a short-lived Docker process started by a systemd timer. There is no resident EveryLot process between posts. Rust can reduce startup and peak application memory; it cannot by itself remove the host's unrelated resident workloads or existing swap use.
2. The production SQLite file is the historical source of truth. Its `lots` table contains 46,944 confirmed Bluesky posts, while `post_deliveries` contains only 1,770 newer confirmations. A cursor based solely on the newer table would lose history. Preserve the current text PIN10 ordering and `lots.posted_bluesky` high-water semantics.
3. The installed deployment script calls `node dist/src/cli/migrate.js`, `node dist/src/cli/audit.js`, and `node dist/src/cli/post-next.js --dry-run`. A native image must accept those invocations before it is promoted, even if it contains no JavaScript runtime.
4. Both environment files, the existing database, the existing session file, and the host units can remain in place. A compatibility executable at `/usr/local/bin/node` can dispatch the known historical CLI arguments to the Rust implementation. Test this through the actual Docker commands rather than assuming an entrypoint shim is equivalent.
5. Production already has schema migration version 1. Normal posting can verify that schema rather than running DDL on every invocation. The initial Rust release does not need a new schema, a new database, re-ingestion, altered journal mode, or a cursor migration.
6. Current tests cover important cursor and composition rules, but do not exercise the remote-success/local-failure delivery boundary. A rewrite should add protocol and failure tests using local fake HTTP services and disposable local databases.

## What was inspected and how

Reviewed all TypeScript modules, CLI entrypoints, SQLite schema, package and Docker configuration, deployment files, GitHub Actions workflows, README documents, and the existing tests. There were no applicable `AGENTS.md` files in the repository or its ancestor directories.

Production inspection used SSH with the provided key. It read operating-system state, targeted systemd properties and unit definitions, Docker image metadata, bounded journal output, file metadata, and SQL queries. Database connections used `mode=ro` plus connection-local `PRAGMA query_only=ON`. No bot invocation, image pull, restart, build, environment edit, journal-mode change, migration, schema edit, or database update was performed.

Environment inspection emitted names and file permissions only. Session inspection emitted JSON field names and file permissions only; credentials and tokens were not printed. The review did not authenticate to Bluesky, request a Street View image, or make a social post.

A consistent logical database snapshot was streamed to the local machine for parity testing using the SQLite shell in `-readonly` mode, `PRAGMA query_only=ON`, and an explicit read transaction around `.dump`. No remote dump or backup file was created. The output was compressed locally:

- Local file: `/tmp/everylot-rust-validation/production-20260918T1452Z.sql.gz`.
- Compressed size: 4,169,992 bytes.
- SHA-256: `9a2287acb9489cbe9165de795b644b05a540dcad682989e8bffa969481ab36fa`.
- The gzip stream was read successfully through its final `COMMIT;`.
- The dump has 606,956 `lots` rows, one migration row, 1,770 delivery rows, and 1,794 run rows. Lease and platform-state tables are empty.
- Snapshot Bluesky high-water PIN10: `1615113015`; next PIN10: `1615113016`.

The snapshot is transient local validation material, not a repository artifact or replacement production database. Further tests should reconstruct separate local copies from it.

## Production topology and resource evidence

| Item | Observation |
| --- | --- |
| Host | Lightsail at `3.142.252.101`, Ubuntu 24.04.3 LTS, x86_64 |
| Kernel | `7.0.0-1010-aws` |
| Host uptime | About 22.5 days |
| Physical memory | 412 MiB reported by `free -m` |
| Idle memory snapshot | 272 MiB used, 139 MiB available |
| Swap | 2,047 MiB total; about 794 MiB used at first inspection |
| Root filesystem | 19 GiB total, 86% used, about 2.7 GiB available |
| Runtime image | `ghcr.io/misterclean/everylotbot-chicago@sha256:fe77599fdef42e357024615a5e361958ec9b21e9f669796a5293234ffb60c521` |
| Image revision | `b1c6594bd5b6bbb51f541766b5a38c46ce5b1c1e`, matching the reviewed repository HEAD |
| Runtime command | `node dist/src/cli/post-next.js`, via the Node image's default entrypoint |
| Runtime user | UID/GID `1000:1000` |
| Container bounds | 192 MiB memory, 64 processes, read-only root, 64 MiB `/tmp`, dropped capabilities |
| Posting timeout | systemd `TimeoutStartSec=10min` |
| Posting schedule | Every 15 minutes during UTC hours `00..07,12..23`; `Persistent=false` |
| Deployment polling | About every five minutes with randomized delay |
| Last observed scheduled run | 14:45 UTC; systemd success, exit status 0, about 11 seconds wall time |
| Recent database run durations | 603 completed runs since September 11; average 8.41 seconds, maximum 56.53 seconds |

The root filesystem is only about 0.7 GiB above the deployer's two-GiB pre-pull floor. A smaller runtime image reduces transfer and storage pressure, but the first Rust rollout must still coexist with retained images and a fresh database backup. The host has other application images and workloads; a Rust rewrite is not authorization to remove them.

The `docker image ls` display reported 286 MB for each retained EveryLot image. `docker image inspect` reported a separate `.Size` value of 62,751,899 bytes on this Docker installation. These are recorded as distinct observed metrics rather than treated as interchangeable uncompressed-size measurements.

Most process swap was attributable to other resident workloads: approximately 285 MiB for `temp-inversion-`, 189 MiB for `python3`, and 185 MiB for `python`. Docker and containerd also have resident and swapped memory. The second `vmstat` sample showed no active swapping. Historical swap allocation alone does not demonstrate ongoing thrashing or an EveryLot leak.

No EveryLot container was running at inspection. Systemd did not retain a useful `MemoryPeak` for the completed process, and its service CPU accounting principally covers the Docker client rather than the separately managed container workload. No OOM messages were found in the available kernel journal, but journal retention limits prevent that from proving none ever occurred. This review did not measure a production application RSS peak and does not claim a measured percentage saving from Rust.

For evidence of savings, benchmark equivalent release binaries against the local production snapshot and fake HTTP responses. Record maximum resident memory, elapsed time, and container memory for audit, dry-run, successful posting, and reconciliation. Keep request data and database cache conditions comparable. Separately observe a naturally scheduled production run when authorized; do not trigger extra live posts for benchmarking.

## Persisted state and compatibility

### Files and ownership

| State | Location and observed properties |
| --- | --- |
| Live database | `/home/ubuntu/bots/everylotbot-chicago/cook_county_lots.db`, 55,967,744 bytes, UID/GID 1000, mode 0664 |
| Active environment | `/etc/everylotbot.env`, root:1000, mode 0640 |
| Historical repository environment | `/home/ubuntu/bots/everylotbot-chicago/.env`, 1000:1000, mode 0600 |
| Bluesky session | `/var/lib/everylotbot/bluesky-session.json`, 1000:1000, mode 0600, 934 bytes |
| Host posting lock | `/var/lib/everylotbot/post.lock`, 1000:1000, mode 0600 |
| Image pointer | `/etc/everylotbot-image.env` |
| Backup directory | `/var/lib/everylotbot/deploy-backups`, root:root, mode 0750; approximately 316 MiB |
| Deployment metadata | `/var/lib/everylotbot/deployments`, root:root, mode 0750 |
| Disposable shadows | `/var/lib/everylotbot/deploy-shadow`, 1000:1000, mode 0750; empty at inspection |

The live service mounts the database directory at `/data` and the state directory at `/state`, then explicitly overrides `DATABASE_PATH` and `BLUESKY_SESSION_PATH` to the mounted files. The working tree's `.env` is not the active container environment. Never replace the active environment from `.env.example` or copy a local `.env` into the image.

The active environment contains the names `GOOGLE_API_KEY`, `BLUESKY_IDENTIFIER`, `BLUESKY_PASSWORD`, `ENABLE_TWITTER`, `ENABLE_BLUESKY`, `PRINT_FORMAT`, `STREETVIEW_PITCH`, `STREETVIEW_ZOOM`, `BLUESKY_SERVICE`, `BLUESKY_SESSION_PATH`, `DATABASE_PATH`, `HTTP_TIMEOUT_MS`, and `LEASE_SECONDS`. It does not include a Cook County token. Posting must not acquire a new requirement for `CHICAGO_DATA_PORTAL_TOKEN`; only administrative import and enrichment require that token.

The repository's historical `.env` additionally contains Cook County and Twitter credential names and old `START_PIN10`/`SEARCH_FORMAT` names. Existing TypeScript does not use those old names. Avoid giving them new behavior that could silently alter the posting cursor.

Docker's `--env-file` preserves surrounding quote characters. Current code deliberately strips one matched quote pair from `PRINT_FORMAT`. Preserve this behavior even when a dotenv parser strips quotes for locally loaded files. Process environment values must continue to take precedence over dotenv values.

The session object has fields `did`, `accessJwt`, `refreshJwt`, `handle`, `email`, `emailConfirmed`, `emailAuthFactor`, and `active`. Deserialize the existing format and keep compatibility with the old program during rollback. Refresh or renew sessions as required, with atomic file replacement and 0600 permissions. Do not log token-bearing response bodies or overwrite this file during audit, migration, or dry-run.

### Database shape and historical history

The database uses SQLite's `delete` journal mode with 4,096-byte pages. No WAL or SHM sidecars were present. It has 13,664 pages and no freelist pages. Keep this configuration unchanged for the initial rewrite.

Existing tables are `lots`, `schema_migrations`, `platform_state`, `post_deliveries`, `bot_runs`, and `bot_leases`. The `lots` definition is:

```sql
CREATE TABLE lots (
  id TEXT PRIMARY KEY,
  address TEXT,
  lat REAL,
  lon REAL,
  posted_twitter TEXT DEFAULT '0',
  posted_bluesky TEXT DEFAULT '0'
);
```

Only implicit primary-key indexes are present. Schema migration version 1 was applied at `2026-08-27T02:45:03.161Z`.

| Statistic at the snapshot | Value |
| --- | ---: |
| Total parcels | 606,956 |
| First PIN10 | `0428206041` |
| Last PIN10 | `3213402011` |
| Confirmed Bluesky references | 46,944 |
| Legacy Bluesky `'1'` sentinels | 0 |
| Nonzero Twitter posting states | 0 |
| First Bluesky PIN10 | `1431213018` |
| Last Bluesky PIN10 | `1615113015` |
| Intentionally skipped before first post | 188,087 |
| Pending gaps between first and last post | 0 |
| Pending after high-water mark | 371,925 |
| New delivery rows | 1,770, all Bluesky and confirmed |
| Maximum recorded delivery attempts | 2 |
| `bot_runs` outcomes | 1,770 posted; 24 failed |
| In-flight/unknown deliveries | 0 |
| Active leases | 0 |
| Platform-start rows | 0 |
| Missing-address sentinels | 875 |
| Pending addressless parcels missing coordinates | 0 |

The inspected database has no malformed PIN10s or NULL addresses, coordinates, or posting-state fields. The schema nevertheless allows NULLs. Tests should include that legacy shape and either handle unusable data consistently or fail safely without publishing.

The string primary key is essential: converting PIN10 to a number would discard leading zeroes and could alter formatting or ordering. The pending marker is the string `'0'`; successful references are strings. Preserve historical values byte for byte.

### Cursor and delivery semantics

- The current platform cursor is `MAX(lots.id)` with that platform's posting state unequal to `'0'`. `platform_state.start_after_id` is a fallback only when there is no confirmed high-water value, followed by the configured fallback start.
- Automatic selection finds the first pending PIN10 strictly above the cursor. It never fills older gaps automatically.
- Explicit `--id` may target an older pending parcel; this is different from normal cursor selection.
- Multiple enabled platforms independently select their next parcel. The invocation chooses the lowest candidate and publishes only to platforms that selected that parcel.
- Twitter requires `TWITTER_START_PIN10` whenever enabled and refuses to retry `unknown` or `publishing` deliveries automatically. Preserve its OAuth 1.0a signing and existing media/post endpoints if the feature remains supported.
- The database lease is named `post-next`, uses a UUID owner and epoch-second expiry, and is acquired in `BEGIN IMMEDIATE`. Release only a lease owned by the current invocation.
- Begin a delivery and persist its retry key before the external post. Atomically update the parcel reference and delivery confirmation after success.
- Bluesky keys are persisted TIDs, not a function of PIN10. Reuse the stored valid key across retries. Query the remote record by that key and check its text before creating a new record.
- A remote write timeout has an uncertain outcome. Keep enough state to reconcile it on the next run instead of issuing an unrelated record key.
- Keep network operations outside database write transactions. Connection timeout settings and short transactions matter on this shared host.

Current code fetches the Google image before checking whether the Bluesky record already exists. Reconciliation can avoid that network request by looking up an existing delivery first, provided text validation, retry-key persistence, and the multi-platform path retain their semantics. This is a useful improvement to cover with a fake-server test.

The largest observed query cost was the current high-water query: a single read-only execution took about 2.34 seconds. `EXPLAIN QUERY PLAN` uses the PIN10 primary-key index; it must traverse a large unposted tail while testing the posting-state column. Rust does not inherently remove this cost. Avoid guessing a cursor from `post_deliveries`; preserve correctness in the first release. An additive partial index could be evaluated separately on a disposable copy if later schema work is authorized.

## Existing behavior worth retaining

### Composition and Street View

Address composition takes the first comma-delimited part, expands direction abbreviations, title-cases street-name words, and stops after the first recognized street-type suffix. The recognized suffix set and exact text/alt wording are observable behavior and already have tests.

Missing addresses are `''`, `'CHICAGO, IL'`, and `', CHICAGO, IL'` after trimming and uppercasing. Those parcels require finite, nonzero, in-range coordinates. Their text is just the PIN10, and their alt text explains the missing common address.

Street View address searches add the Chicago suffix only when absent. Coordinate fallback is used only for missing addresses. Preserve the current request parameters, including radius for both address and coordinate searches, outdoor source, dimensions, field of view, pitch, zoom, and error-code request. Defaults include a 500-meter radius. Changing image dimensions or composition should be a separately visible product decision, not an incidental effect of changing languages.

Current image handling buffers the full compressed response, retries up to three times, and checks HTTP status, image content type, and nonzero length. It does not decode pixels. Rust should also avoid pixel decoding, maintain one bounded compressed buffer, and upload by borrowing that buffer. Reject oversized responses while reading rather than after unbounded allocation. The current upstream Bluesky image lexicon allows 2,000,000 bytes per image; the old one-MB assumption is stale. [Bluesky image lexicon](https://raw.githubusercontent.com/bluesky-social/atproto/main/lexicons/app/bsky/embed/images.json).

The feed-post lexicon requires TID keys and limits text to 300 graphemes and 3,000 bytes. Current short parcel text fits comfortably, but configurable templates should be checked before remote writes. [Bluesky post lexicon](https://raw.githubusercontent.com/bluesky-social/atproto/main/lexicons/app/bsky/feed/post.json).

TIDs are 13-character base32-sortable identifiers with a restricted first character, a microsecond timestamp, and clock identifier bits. Preserve existing keys exactly; generating a new key for each retry defeats reconciliation. [AT Protocol TID specification](https://atproto.com/specs/tid).

### Import and centroid enrichment

Administrative import streams Cook County CSV into a temporary staging table, validates PIN formats, rejects an empty valid result, and merges addresses without clearing posting references or coordinates. A later blank source address must not replace an existing nonblank address. Keep source rows streamed and transactions bounded; avoid holding all 606,956 rows in a Rust collection.

Centroid enrichment currently selects all unposted missing-address parcel IDs, requests batches of at most 100, considers newest source years first, and updates coordinates only while the parcel still satisfies the pending/missing-address predicate. It keeps candidate IDs and matched coordinates in memory. The current 875 candidates are small, but batching the selection and updates would make the memory bound independent of county size. Preserve newest-valid-coordinate selection and avoid advancing posting state.

Neither maintenance command is part of routine posting or deployment. They must not be invoked automatically as part of a language migration.

## Deployment and rollback contract

The installed deployer, app service, and app timer have byte-for-byte hashes matching their repository versions. The older host checkout is at `1a6c31ce2532d3342a4b8b8d6456d5e46c1e3554`; the image's OCI revision is the authoritative runtime version.

CI currently tests/builds TypeScript, builds an amd64 image, then smoke-tests migration, audit, and dry-run with no network and a read-only container root. The release workflow runs after successful main-branch CI, checks out that exact tested commit, builds and attests an image, then promotes its digest through the production environment. The host poller resolves that tag and runs by digest. The Rust CI replacement must preserve the smoke-test boundary and release trigger.

The host deployer checks free disk space, pulls the candidate, requires `linux/amd64` and a valid 40-character OCI revision label, stops the posting timer, and defers if the posting service is active. It backs up the live database, integrity-checks the backup, creates a shadow, migrates only the shadow, compares schema inventories, and runs candidate audit and dry-run with network disabled. It then atomically changes the image pointer and retains deployment metadata and the database backup. The posting timer is restored by a cleanup trap.

For an unchanged host deployer, a Rust image must support:

```text
docker run <existing bounds and mounts> <image>
docker run <shadow bounds and mounts> <image> node dist/src/cli/migrate.js --database /shadow/<file>
docker run <shadow bounds and mounts> <image> node dist/src/cli/audit.js --database /shadow/<file>
docker run <shadow bounds and mounts> <image> node dist/src/cli/post-next.js --database /shadow/<file> --dry-run
```

A native `node` compatibility executable/symlink is reasonable if it recognizes only the known historical command forms, errors on unknown JavaScript paths, and dispatches to the same tested Rust command handlers. Put it on `PATH`. Use an image `CMD` arrangement that allows Docker's explicit command override; an unconditional executable `ENTRYPOINT` can accidentally receive the literal `node` token as an unexpected application argument.

The runtime must work as UID/GID 1000 without changing ownership, with an unwritable root filesystem and only the existing mounted directories and `/tmp` writable. Include TLS trust roots or a deliberate supported Rust TLS root strategy. DNS, time formatting, SQLite locking, and session writes must work in that image, not merely on the development machine.

The deployer's schema inventory compares object names and column types/defaults, but does not fully compare index definitions, constraints, triggers, or historical row contents. Passing its shadow check alone does not establish data compatibility. Local full-snapshot parity tests should cover both schema and rows.

Rollback moves only the image pointer. Never restore an old database automatically when rolling back code; posting history may have advanced. Keep the previous TypeScript image able to read Rust-written delivery/session state. An additive new state value outside the existing CHECK constraints or a changed session format would break that guarantee.

## Suggested implementation priorities

1. Build one small command-line binary with explicit commands and the narrow legacy invocation adapter. Keep the scheduled lifecycle as one invocation per parcel.
2. Open one SQLite connection and one blocking HTTP client per run; avoid unnecessary runtimes, pools, background tasks, and duplicate media buffers. Optimize release builds and measure before adding allocator or unsafe-code complexity.
3. Set a modest connection-local SQLite cache bound when useful. SQLite documents that negative `cache_size` values express a KiB-based limit and apply only to the connection; do not confuse this with changing persistent journal or page-size settings. [SQLite cache documentation](https://www.sqlite.org/pragma.html#pragma_cache_size).
4. Make audit/dry-run genuinely read-only and network-free. Open existing databases without a create flag; a mistyped path must not generate an empty database. `mode=ro` is an actual read-only open mode, whereas `immutable=1` additionally disables locking and change detection and is inappropriate for a changing live database. [SQLite URI documentation](https://www.sqlite.org/uri.html).
5. Bound every HTTP request and response. Preserve the existing session's account identity, refresh behavior, retry key, error classification, and post formatting before pursuing protocol shortcuts.
6. Use structured errors that distinguish pre-publication failures from uncertain remote writes. Redact credential-bearing URLs and tokens; generic HTTP-library errors may include the Google API key in a request URL.
7. Keep the first release free of persistent-schema changes. Avoid enabling WAL, vacuuming, rewriting rows, deriving a replacement cursor, or normalizing historical strings during startup.
8. Test the exact container command interface and full historical snapshot locally. Production verification should follow only after the candidate and its compatibility evidence are reviewable.

## Validation required before release

- Native and legacy command paths select `1615113016` from the captured snapshot and produce identical text/alt output to the existing implementation.
- Audit counts, skip/gap behavior, platform-specific selection, explicit IDs, and quoted configuration match existing expectations.
- Audit and dry-run leave both database content and schema unchanged and never reach a network endpoint.
- Migration is idempotent on existing v1 state and does not rebuild or replace `lots`.
- A fake remote success followed by local interruption resolves the existing Bluesky record on retry with the same stored key.
- An uncertain Twitter write remains blocked until explicitly reconciled.
- Existing session JSON is readable; refreshed session JSON remains readable by the old implementation; no session writes occur in offline commands.
- Response bounds, HTTP timeouts, lease conflicts, missing databases, invalid configuration, missing coordinates, expired credentials, record-text mismatches, and SQLite busy/errors fail safely.
- Import and centroid fixtures preserve posting references and reject unsafe empty/malformed input.
- Runtime tests use the actual amd64 container, UID 1000, read-only root, no-network shadow configuration, current memory limit, and the exact legacy command forms.
- Release memory measurements are reported as measurements of specific scenarios, not extrapolated from host swap or image size.

No production change or deployment was performed during this research.

## Follow-up: native session and retry review

The first Rust draft was inspected statically after the research above. Its renewed lease, durable key, confirmation transaction, and preservation of uncertainty through failed reconciliation improve the old failure boundary. No authenticated requests were made to validate the draft.

The absence of `didDoc` in the persisted production session does not prove the old client always used `bsky.social` for repository requests. The exact `@atproto/api` 0.20.42 source refreshes a resumed session, obtains the DID document from refresh/get-session responses, and keeps the chosen PDS URL separately in memory. It persists only the ordinary session fields. The same source explicitly allows falling back to the configured service because Bluesky's entryway proxies requests. [Pinned CredentialSession implementation](https://raw.githubusercontent.com/bluesky-social/atproto/%40atproto%2Fapi%400.20.42/packages/api/src/atp-agent.ts).

Bluesky's documentation identifies `https://bsky.social` as its entryway, and its migration announcement states that existing bot clients can continue using that endpoint with proxying. Consequently, using `bsky.social` for authenticated repository operations is supported for Bluesky-hosted accounts; it is not a demonstrated production outage. Routing to the returned canonical PDS still matches the SDK and supports endpoint changes. [API hosts documentation](https://github.com/bluesky-social/bsky-docs/blob/main/docs/advanced-guides/api-directory.mdx), [Bluesky migration announcement](https://github.com/bluesky-social/atproto/discussions/1832).

Two draft parity issues were reported to the implementing agent for correction and tests:

- The initial draft skipped server validation when the stored access token's decoded expiry was in the future and had no refresh-on-401 path. A token rejected before its local expiry could therefore block several scheduled runs despite a usable refresh token. Refreshing on resume follows the old SDK. A successful refresh must return the same DID as the stored session before any new session is persisted or a post is attempted.
- The first template parser treated literal/nested braces differently from JavaScript's `/\{([^{}]+)\}/g`: `{}` should remain unchanged and `{{id}}` should become `{<PIN10>}`. Preserve those cases or document an intentional format change.

These are findings about an evolving draft, not claims that they remain in the final implementation. The implementing agent owns the fixes and validation.

The implementing agent corrected all four reported parity findings: forced
session refresh with DID verification, literal/nested format braces, durable
Twitter start state, and removal of stale DID-document routing. Each has a
regression test. See [the final validation results](rust-validation.md).
