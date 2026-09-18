# Rust and native deployment plan

## Compatibility

Port the entire bot to one synchronous Rust executable, keeping the original
SQLite schema, PIN10 text ordering, per-platform high-water marks, persisted
keys, sentinel values, history, and session JSON. Posting must not run DDL.
Audit and dry-run are read-only and network-free. Explicit migration is a no-op
on the current v1 schema. Imports stream through temporary on-disk staging.

## Native CI/CD

1. Required CI checks formatting, Clippy, application regressions, deployment
   failure/rollback tests, and a native smoke test on Ubuntu 24.04 x86_64.
2. Main CI uploads the tested executable and SHA-256 manifest. Release attests
   and publishes that exact artifact under `app-<commit>`, then promotes the
   `production.json` pointer only if main still points at the tested commit.
3. The production environment remains branch-restricted, with no separate
   reviewer approval, as requested. Lightsail polls every five minutes.
4. Deployment downloads and verifies the candidate, serializes with posting,
   makes a new read-only-source SQLite backup, validates a shadow in a
   network-disabled/read-only systemd sandbox, checks the live database hash,
   and atomically switches `/opt/everylotbot/current`. It never migrates or
   replaces the live database, writes credentials, runs live posts, or deletes
   logs. Rollback changes code using the same validation path.

## Shared Petit

Reuse the installed Petit 0.2.0 executable at pinned revision
`170dee43be847b29bc065f4265a4b3e16d4fb7fd`, copied into `/opt/petit-bots`.
A single `petit-bots.service` schedules both jobs and reuses the existing
`/var/lib/petit-chicago-bikeshare-bot/history.sqlite3` history database.
EveryLot keeps its existing quarter-hour UTC schedule and separate worker lock.
Its wrapper clears inherited bikeshare configuration and reads the unchanged
EveryLot environment file with native path overrides.

A persistent drop-in turns the old bikeshare service into a compatibility unit
bound to the shared scheduler. Its existing updater and health timer continue
working without creating a second scheduler. Bike job paths follow the existing
release symlink. Neither bot's routine code deploy restarts the shared scheduler.

## Initial cutover

Open the PR and pass CI. Back up host configuration; install the native poller
and shared scheduler with EveryLot disabled while the old posting timer runs.
Preserve bike history and logs. Merge after CI, let Release publish and the host
validate/install the native executable, then drain both jobs, disable the old
EveryLot posting timer, and enable EveryLot in Petit. Verify the next ordinary
scheduled post and cursor advance. Do not trigger an extra live post to test.

The user authorized host preparation, merge, deployment, and fully automatic
future main deployments on 18 September 2026. The earlier read-only research
restriction applied to the investigation phase, now completed.
