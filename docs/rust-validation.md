# Rust migration validation

Validated locally on 18 September 2026 on branch `codex/rust-memory-refactor`.
The prior implementation was commit `b1c6594bd5b6bbb51f541766b5a38c46ce5b1c1e`.
The initial research and parity checks were read-only. The subsequent native
rollout is authorized and recorded below as it proceeds.

## Completed scope

All five runtime commands are Rust: `post-next`, `audit`, `migrate`, `ingest`,
and `enrich-centroids`. TypeScript sources, Node package manifests, and the old
test/build configuration have been replaced. CI builds and checks Rust, smoke-tests the native executable, and tests
deployment failure and rollback behavior. Release, rollback, and host deployment
now use native Linux executables, with one shared Petit scheduler.

The Astra xHigh research agent reviewed the current code, production host,
bounded logs, schema, delivery state, deployment contract, and the Rust draft.
Its findings and recommended future work are in [rust-research.md](rust-research.md).
The implementation plan is in [rust-plan.md](rust-plan.md).

## Historical database compatibility

The research agent streamed a consistent SQLite logical dump through a
read-only transaction over SSH, compressed locally. It created no remote file
and issued no remote writes. Local reconstruction contains 606,956 parcels,
46,944 confirmed Bluesky posts, and the complete ancillary history.

On that snapshot, old and new audit JSON matched exactly as parsed objects:

- first confirmed Bluesky parcel: `1431213018`;
- last confirmed parcel: `1615113015`;
- skipped earlier parcels: 188,087;
- gaps inside the confirmed run: zero;
- remaining parcels after the cursor: 371,925;
- next parcel: `1615113016`.

Old and new dry-run output matched for platform, ID, text, and alt text:
`4648 West Gladys Avenue`.

The reconstructed database SHA-256 remained
`5ed196bf66d8b46f0072b3fddba34f1a6a728ecb6d39f80ab7a5ba34d918c3c5`
before and after native audit, dry-run, repeated measurements, and `migrate`.
The existing v1 migration is a no-op. Tests additionally verify that posting
refuses an unmigrated database rather than applying startup DDL.

During the initial investigation, a Linux amd64 container also produced identical
read-only audit/selection results under 64 MiB. That container implementation
was superseded by the requested native deployment; Docker is no longer required.

## Memory measurements

Release Rust 1.94.0 and Node 24.18.0 ran against the same full snapshot on macOS
arm64. `/usr/bin/time -l` measured maximum resident set size, converted from
bytes to MiB. The table reports the median of three sequential invocations per
command; normal OS caching was allowed. No production credentials or external
requests were used.

| Command | Node peak RSS | Rust peak RSS | Reduction | Node elapsed | Rust elapsed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Audit | 65.36 MiB | 10.36 MiB | 84.2% | 0.87 s | 0.62 s |
| Post dry-run | 120.47 MiB | 10.33 MiB | 91.4% | 0.36 s | 0.09 s |

Dry-run exercises selection and composition but not TLS, session refresh,
image transfer, or remote posting. These numbers are not claims about actual
Linux production posting peaks, host-wide swap reduction, or guaranteed
latency. Other host workloads account for much of the observed swap. The
Rust binary uses synchronous HTTP, bounded response sizes, one compressed
image buffer, a connection-local 2 MiB SQLite cache, and disk-backed import
staging to keep runtime memory bounded beyond the measured offline paths.

Raw local measurement evidence is in
`/tmp/everylot-rust-validation/memory-results.json`. Snapshot and raw logs stay
outside the repository and are not deployed.

## Automated checks

- All 22 original TypeScript tests passed before the old implementation was removed.
- 34 Rust regression tests pass in debug and release builds, including mock HTTP publication, refresh,
  account-DID mismatch, stale PDS metadata, existing-record reconciliation,
  uncertain writes, conflicting records, invalid durable keys, atomic local
  confirmation, lease exclusion/expiry, independent platform cursors,
  Twitter OAuth signing/start persistence, read-only commands, import staging,
  CSV quoting/BOM/newlines, failed later import pages, centroid selection,
  response bounds, and response-body timeout.
- `cargo fmt --all -- --check` passes.
- `cargo clippy --all-targets --all-features --locked -- -D warnings` passes.
- `cargo build --release --locked` passes.
- Native smoke tests preserve fixture bytes and reject missing databases.
- Six deployment regression tests run on Linux CI: successful deployment,
  idempotence, rollback, checksum rejection, validation rejection, absent release,
  malformed commit, and mismatched rollback identity (some combined).
- Shell syntax, ShellCheck, and Git whitespace checks are CI gates.

Rust is compiled in CI, never on the small production host. The executable uses
bundled WebPKI roots and does not need Node or Docker.

## Intentional behavior improvements

- Reconcile an existing Bluesky record before requesting another image.
- Renew and verify the posting lease before remote publication, and bound
  each request to at most one quarter of the lease lifetime.
- Retain uncertain delivery state after reconciliation errors or local
  confirmation failure; block invalid uncertain keys for manual review.
- Refresh saved sessions and reject account identity changes; discard stale
  PDS metadata when refresh no longer supplies it.
- Bound image responses at the current 2,000,000-byte Bluesky limit, JSON
  responses at 1 MiB (4 MiB for centroid pages), and streaming CSV pages at
  16 MiB. Requests above these limits fail before publication/final import.
- Default CSV pages are 5,000 rows instead of 50,000. Users can still request
  up to 50,000 within the response bound.
- Centroid results report a total missing count and at most 100 missing IDs,
  avoiding an unbounded result array.

No persistent indexes, columns, tables, journal modes, historical values,
posting cursors, or environment variables were changed for this migration.
Normal future posting will continue updating the existing history tables.

## Native rollout verification

The user requested automatic deployment after required CI, host preparation,
merge, and production rollout. GitHub's separate production reviewer gate will
be removed while retaining protected-branch restrictions. The poller validates
a shadow without network or writable persistent mounts, verifies unchanged live
DB bytes, and changes only an executable symlink. Environment files and existing
logs are retained. No deployment invokes a live post or schema migration.

The next ordinary scheduled post is the live verification point: authentication,
image delivery, exactly one cursor advance, successful exit, and actual Linux
memory. Production results are appended after rollout. Local dry-run numbers
are not substituted for production memory evidence.

Host preparation completed at 16:19 UTC on 18 September 2026. Both Petit job
configurations validate; EveryLot is disabled in Petit pending the native
release, and its original timer is still active. The shared scheduler reuses
all existing history. Restarting the bikeshare compatibility service retained
the shared scheduler PID. SHA-256 checks confirmed both EveryLot environment
files, the bikeshare environment, bikeshare DB, and existing bikeshare log are
unchanged. Host configuration backups are under
`/root/everylot-native-prep-20260918T161909Z`.

GitHub's production reviewer requirement was removed as requested; the protected
branch restriction and required main `test` check remain enabled.
