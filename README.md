# EveryLot Chicago

EveryLot Chicago is a TypeScript bot that posts Google Street View images of Cook County parcels to Bluesky in ascending PIN10 order.

The application uses SQLite for parcel data and durable delivery state. It supports safe retries, deterministic Bluesky record keys, persisted sessions, dry runs, and addressless parcels backed by Cook County parcel centroids.

## Requirements

- Node.js 24
- npm
- SQLite 3
- A Cook County Open Data app token
- A Google Street View Static API key
- A Bluesky account and app password

## Setup

Install dependencies and create a local configuration file:

```bash
npm ci
cp .env.example .env
```

Create an empty parcel database from the included schema:

```bash
sqlite3 cook_county_lots.db < schema.sql
```

Fill in `.env`, then build and import parcel addresses:

```bash
npm run build
npm run ingest -- --year 2023 --city CHICAGO
npm run enrich-centroids
```

The importer stages and validates Cook County data before updating `lots`. It preserves posting state and does not replace an existing address with a blank source address. Centroid enrichment updates only unposted parcels without usable addresses and leaves addresses and posting state unchanged.

## Configuration

The application reads configuration from environment variables and an optional `.env` file. See [.env.example](.env.example) for the full list.

Required for Bluesky posting:

```dotenv
ENABLE_BLUESKY=true
ENABLE_TWITTER=false
CHICAGO_DATA_PORTAL_TOKEN=...
GOOGLE_API_KEY=...
BLUESKY_IDENTIFIER=your-handle.bsky.social
BLUESKY_PASSWORD=...
DATABASE_PATH=cook_county_lots.db
```

Keep credentials and Bluesky session files outside version control. If Twitter is enabled, `TWITTER_START_PIN10` is required to prevent accidental historical backfills.

## Commands

Build the application before running its compiled commands:

```bash
npm run build
npm run audit
npm run post-next -- --dry-run
npm run post-next
```

Available `post-next` options:

- `--database <path>` overrides `DATABASE_PATH`.
- `--id <PIN10>` targets a specific pending parcel.
- `--platform bluesky|twitter|all` restricts enabled platforms.
- `--dry-run` selects and composes a post without database writes or network requests.
- `--verbose` enables debug logging.

When an addressed parcel is selected, the configured `PRINT_FORMAT` controls its post text. For a parcel without a common address, the post contains only its PIN10 and uses the parcel centroid for Street View. The image alt text explicitly notes that the parcel does not have a common address.

## Scheduling and deployment

Run `npm run post-next` from any scheduler that supports non-overlapping jobs, such as cron, systemd, or a container platform. Each invocation selects at most one parcel. The application also takes an expiring SQLite lease to prevent overlapping publishers.

Build artifacts are written to `dist/`. A deployment needs `dist/`, production `node_modules/`, `package.json`, and `package-lock.json`, plus externally managed environment variables, credentials, session storage, and the SQLite database.

## Delivery safety

The bot chooses the next pending PIN10 above the platform's confirmed high-water mark. It does not automatically backfill older gaps. Successful post references remain in the `lots` table, while additive application tables track runs, leases, and delivery reconciliation.

Bluesky posts use deterministic AT Protocol record keys. If publication succeeds remotely but local confirmation is interrupted, the next run resolves the existing record before attempting another post.

## Development

```bash
npm run check
npm test
npm run build
```

Source code lives in `src/`, and tests live in `tests/`.

## License and credits

GPL-3.0. This project is based on Neil Freeman's original `everylotbot` and was adapted for Cook County data and modern social platforms.
