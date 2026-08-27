# TypeScript migration specification

Status: implemented on `codex/refactor`; not deployed to production.

## Objective

Replace the Python posting and ingestion commands with a strict TypeScript application while preserving the live Lightsail host, SQLite database, Bluesky account, post format, ordering, and schedule. Python stays available as the rollback implementation during a seven-day observation period.

## Production baseline

Read-only discovery on July 26, 2026 established:

- Ubuntu 24.04 x86_64 with 416 MiB RAM, 2 GiB swap, and approximately 2.6 GiB free disk.
- PM2 invokes the Python oneshot 80 times per day, every 15 minutes in UTC hours `00–07` and `12–23`.
- The live database had 606,956 lots, 44,086 confirmed Bluesky URLs, no Twitter posts, and passed `PRAGMA integrity_check`.
- Confirmed Bluesky rows formed a continuous run from `1431213018` through the then-current cursor. There were 188,087 deliberately unposted rows below that start.
- The repository source matched production revision `1a6c31c`; only the database and untracked PM2 file differed.
- The checked-in/untracked ecosystem schedule was stale relative to PM2's loaded schedule.
- The host had no EveryLot-specific backup and no PM2 log rotation. Docker images for another bot consumed approximately 2.88 GB.

All cursor values are historical observations. Cutover must query the current values again.

## Runtime design

- Node.js 24 LTS, installed alongside rather than replacing the host's EOL NVM Node 23.
- Ahead-of-time compiled ESM JavaScript; no build occurs on Lightsail.
- Official `@atproto/api`, built-in `node:sqlite`, native `fetch`, dotenv, and Zod.
- Direct host execution under a systemd oneshot and timer; no additional Docker image.
- A 96 MiB old-generation heap limit and 192 MiB systemd memory ceiling protect the small host.
- JSON logs go to journald and exclude request URLs containing API keys.

## Compatibility invariants

1. `lots` and every existing row/status value remain unchanged by schema migration.
2. PIN10 stays a string, preserving leading zeroes and lexical ordering.
3. Automatic selection starts strictly above `MAX(id)` with a nonzero platform status.
4. Zeros below the high-water cursor are never automatically backfilled.
5. One invocation selects at most one lot and publishes at most once to each pending platform.
6. A confirmed public reference is written to the legacy platform column.
7. A failed post does not advance that platform's cursor.
8. Dry-run performs no database writes, HTTP requests, login, upload, or publication.
9. Twitter remains disabled. Enabling it requires an explicit `TWITTER_START_PIN10`.
10. Initial cutover preserves post text, ALT text, `1000x1000` image size, pitch `11.55`, zoom `.9`, and the legacy Street View location string.

## Additive schema

- `schema_migrations`: applied migration versions.
- `platform_state`: explicit starting cursor for a platform without legacy confirmations.
- `post_deliveries`: state, deterministic key, attempt count, error, and confirmed reference per lot/platform.
- `bot_runs`: invocation outcome and selected lot.
- `bot_leases`: expiring application-level overlap guard.

The systemd command additionally uses `flock`. The database lease protects manual or alternative invocations that bypass systemd.

## Delivery state machine

```text
selected -> publishing -> confirmed
                       -> failed
                       -> unknown
```

Bluesky records use `everylot-<PIN10>` as their deterministic AT Protocol record key. A retry first resolves that record: if it exists with matching text, SQLite is reconciled instead of creating a duplicate. A secondary platform in `publishing` or `unknown` is never blindly retried because its API may not provide equivalent reconciliation.

Sessions are written atomically with mode `0600` and resumed on later runs. A fresh login occurs only when no usable session is available.

## Safe ingestion

Cook County CSV pages are parsed as a stream into a temporary staging table. The first PIN14 for each PIN10 wins. A validated, nonempty staging set is then upserted into `lots` in one transaction. Existing `posted_bluesky` and `posted_twitter` fields are never part of an update. Rows missing from a source refresh are retained.

Changing the configured source year is outside runtime cutover scope.

## Deployment and rollback

Deployment is a scheduler handoff during the existing `08:00–11:59 UTC` gap:

1. Pass CI, audit, and dry-run against production.
2. Capture current PM2 state, cursor, next lot, disk/memory, Git state, and integrity.
3. Complete a Lightsail snapshot and SQLite `.backup`; verify the backup.
4. Remove only EveryLot from PM2.
5. Apply additive migrations and confirm the next PIN10 did not change.
6. Enable the non-persistent systemd timer and let a normal scheduled boundary perform the first post.
7. Verify exactly one public post, one database URL, one cursor advance, and database integrity.

Rollback disables systemd first and restores `deploy/legacy-everylotbot.pm2.cjs`, which contains the schedule actually observed in PM2. Because TypeScript maintains the legacy status column, Python normally resumes without restoring SQLite. An unknown delivery must be reconciled while both schedulers remain disabled. Database restore is reserved for corruption.

The executable command sequence and verification checklist are maintained in `deploy/README.md`.

## Acceptance criteria

- TypeScript and Python identify the same cutover PIN10.
- The timer validates and produces the intended 80 daily boundaries.
- No selection occurs below the production high-water cursor.
- No duplicate, skipped confirmed, or burst catch-up posts occur.
- Failures return nonzero and remain recoverable.
- The database remains valid and Python-compatible.
- Runtime stays within memory and disk guardrails.
- All first-day invocations and seven daily totals reconcile with public posts and database confirmations.
