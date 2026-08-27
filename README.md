# EveryLot Chicago

EveryLot Chicago posts a Google Street View image of each Cook County property lot to Bluesky, in ascending PIN10 order. The production bot runs on an AWS Lightsail instance every 15 minutes during its configured posting hours.

The repository now contains a TypeScript replacement for the original Python application. Python remains present solely for production rollback until the TypeScript service completes its observation period.

## Production safety contract

The database is the durable production state. The bot finds the largest confirmed platform-specific PIN10 and selects the next pending row above it. It intentionally does **not** fill pending rows below that high-water mark: production began at PIN10 `1431213018`, leaving 188,087 earlier rows deliberately skipped.

The rewrite preserves the legacy `lots` schema and continues storing public post references in `posted_bluesky` or `posted_twitter`. This allows immediate rollback to Python. New migrations are additive and provide run history, delivery reconciliation, and a database lease.

## Requirements

- Node.js 24 LTS
- npm
- The existing SQLite database
- Google Street View API key
- Bluesky handle and app password

Install and verify:

```bash
npm ci
npm run check
npm test
npm run build
```

## Configuration

Copy `.env.example` to `.env` for local use. Production uses `/etc/everylotbot.env` rather than a repository file.

Required for the current Bluesky deployment:

```dotenv
ENABLE_BLUESKY=true
ENABLE_TWITTER=false
GOOGLE_API_KEY=...
BLUESKY_IDENTIFIER=everylotchicago.bsky.social
BLUESKY_PASSWORD=...
BLUESKY_SESSION_PATH=var/bluesky-session.json
DATABASE_PATH=cook_county_lots.db
PRINT_FORMAT={address}
STREETVIEW_PITCH=11.55
STREETVIEW_ZOOM=.9
STREETVIEW_RADIUS_METERS=500
```

Twitter remains disabled in production. Enabling it also requires all four OAuth 1.0a credentials and an explicit `TWITTER_START_PIN10`; the application refuses to enable Twitter without a starting cursor so it cannot accidentally backfill historical lots.

## Commands

Build first, then use the compiled commands:

```bash
npm run build
node dist/src/cli/audit.js
node dist/src/cli/post-next.js --dry-run
node dist/src/cli/post-next.js
```

Options for `post-next`:

- `--database <path>` overrides `DATABASE_PATH`.
- `--id <PIN10>` intentionally targets a specific pending lot.
- `--platform bluesky|twitter|all` restricts enabled platforms.
- `--dry-run` selects and composes without database writes, API authentication, image download, or publication.
- `--verbose` enables debug logging.

Scheduled failures exit nonzero. The next invocation retries the same unconfirmed lot. A database lease and the deployment-level `flock` prevent overlapping publishers.

## Bluesky delivery behavior

New posts use a deterministic AT Protocol record key, `everylot-<PIN10>`. If a request succeeds remotely but local confirmation is interrupted, the next run resolves that record before writing again. Bluesky sessions are persisted with restrictive permissions and resumed, avoiding a fresh login for every one of the 80 daily executions.

## Safe Cook County import

The importer stages and validates paginated CSV data, then upserts addresses while preserving all post status fields. It never drops `lots`.
Rows without a property street address no longer overwrite a previously repaired address.

```bash
node dist/src/cli/ingest.js --year 2023 --city CHICAGO
node dist/src/cli/enrich-centroids.js
```

The centroid enrichment command updates only unposted parcels that lack a usable street address. It selects the latest available Cook County Parcel Universe centroid for each PIN10 while leaving addresses and posting state unchanged.

When a selected lot has no street address but does have a centroid, the post text is its ten-digit PIN10 only. Street View searches for an outdoor panorama within `STREETVIEW_RADIUS_METERS` of the centroid and lets Google aim the camera back toward the requested parcel location. Addressed parcels retain the existing address text and image lookup behavior.

Changing the production tax year is a separate data migration and must not be combined with the TypeScript runtime cutover.

## Deployment

CI builds and tests on Ubuntu x86_64, then produces a versioned production artifact with compiled JavaScript and production dependencies. This avoids compiling TypeScript or resolving packages on the 416 MiB Lightsail host.

The architecture and invariants are recorded in [MIGRATION_SPEC.md](MIGRATION_SPEC.md). The exact production cutover, verification, and rollback procedure is in [deploy/README.md](deploy/README.md). The systemd timer matches the schedule observed in PM2 rather than the stale schedule in the original untracked production ecosystem file.

No deployment is performed merely by merging this branch.

## Project structure

```text
src/
  cli/             audit, ingest, and post-next entry points
  domain/          lot types, address cleanup, post composition
  platforms/       Bluesky and optional Twitter publishers
  services/        Google Street View client
  app.ts            one-run orchestration
  config.ts         validated environment configuration
  db.ts             SQLite compatibility and delivery state
  ingestion.ts      safe Cook County staging/upsert
tests-ts/           TypeScript regression and integration tests
deploy/             systemd units, production environment example, runbook
everylot/           legacy Python rollback implementation
```

## Operational logging

The TypeScript service emits compact JSON to stdout/stderr for journald. Successful HTTP calls are not logged individually, and secret-bearing request URLs are never written to logs. Each posting event includes its run ID, PIN10, platform, outcome, and confirmed public reference.

## License and credits

GPL-3.0. This project is based on Neil Freeman's original `everylotbot` and was adapted for Chicago property data and modern social platforms.
