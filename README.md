# EveryLot Chicago

EveryLot Chicago is a Rust command-line bot that posts Google Street View images of Cook County parcels to Bluesky in ascending PIN10 order. Optional Twitter posting is also supported.

One small synchronous executable handles posting, read-only audits, CSV ingestion, and centroid enrichment. SQLite remains the historical source of truth: existing parcel records, posting references, delivery keys, and cursors work without conversion.

## Setup

Use Rust 1.94 and SQLite 3. Build the native executable:

```bash
cargo build --release --locked
cp .env.example .env
# New installations only. Never recreate or reimport the production database.
sqlite3 cook_county_lots.db < schema.sql
target/release/everylotbot migrate
target/release/everylotbot ingest --year 2023 --city CHICAGO
target/release/everylotbot enrich-centroids
```

The bot reads process environment variables before optional `.env` values. Use `--env-file <path>` to load an explicit credential file. See [.env.example](.env.example). Posting needs Google and enabled-platform credentials; the Cook County token is required only for import/enrichment. Keep the database and session file outside version control.

## Commands

```bash
target/release/everylotbot audit
target/release/everylotbot post-next --dry-run
target/release/everylotbot post-next
```

All commands accept `--database <path>` and `--verbose` (`-v`). Posting also accepts `--id <PIN10>` and `--platform bluesky|twitter|all`. Explicit `--id` can target an older pending parcel. Automatic posting never backfills parcels below a platform's confirmed high-water mark. Twitter requires `TWITTER_START_PIN10` when enabled.

Audit and dry-run open SQLite read-only, make no network requests, and do not write session files. Posting validates the existing v1 schema and never automatically migrates. `migrate` creates only the same additive tables used by the previous application; on an existing v1 database it performs no writes. A missing database path is an error, never an implicit empty database.

`ingest` accepts `--year`, `--city`, and `--batch-size` (default 5,000; maximum 50,000). It streams CSV into disk-backed temporary staging and applies the final merge only after every page succeeds. It preserves existing coordinates, posting references, and nonblank addresses. CSV pages are limited to 16 MiB; reduce the batch size if a source page exceeds that bound.

`enrich-centroids` accepts `--batch-size` (default 75; maximum 100). It stages candidates and matches in SQLite, requests bounded batches, and updates only parcels still pending on Bluesky with no usable address. Its result includes `missingCount` and up to 100 missing IDs, with `missingTruncated` indicating a larger set. Coordinates and posting state are never changed by a routine posting run.

## Delivery and memory

The bot stores a valid Bluesky TID before publishing, checks that record before downloading an image, and reconciles interrupted writes using the same key. Confirmation updates the parcel and delivery row atomically. Unknown Twitter outcomes require manual reconciliation. Invalid stored keys on uncertain Bluesky deliveries also block safely instead of generating a new key.

Sessions retain the legacy JSON format, refresh before use, validate account identity, and are atomically replaced with mode 0600. HTTP failures omit request URLs and token-bearing response bodies. Requests have deadlines; compressed images are capped at 2 MB and are uploaded without pixel decoding. SQLite uses a connection-local 2 MiB page cache. No async runtime, resident daemon, or county-wide in-memory parcel collection is required.

## Deployment

GitHub Actions builds a native Linux executable after `CI / test` succeeds on `main` and automatically promotes the exact tested artifact. Lightsail polls the release manifest and switches `/opt/everylotbot/current` atomically after checksum, backup, and read-only shadow validation. Docker is not required.

One shared [Petit](https://github.com/PedramNavid/petit) service schedules EveryLot and the Chicago Bikeshare bot, preserving the existing Petit run history. EveryLot keeps its quarter-hour schedule and uses the original database, credential file, and session path. Deployments never migrate or replace the parcel database or remove existing logs. Normal scheduled posts continue advancing history.

See [deployment procedures](deploy/README.md), [research](docs/rust-research.md), and [migration plan](docs/rust-plan.md). Rollbacks change only application code, never restore old posting history.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --locked
cargo build --release --locked
bash scripts/native-smoke.sh
python3 -m unittest discover -s tests -v
```

Tests use disposable databases and local mock HTTP servers. They never use production credentials or publish real posts. All application code and regression tests live under `src/`.

## License and credits

GPL-3.0. Based on Neil Freeman's original `everylotbot`, adapted for Cook County and modern social platforms.
