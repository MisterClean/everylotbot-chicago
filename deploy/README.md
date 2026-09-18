# Native Lightsail deployment and shared Petit

Merging to protected `main` triggers CI. After `test` passes, Release downloads
that exact Ubuntu 24.04 x86_64 executable, verifies and attests it, publishes
`app-<commit>`, and updates the GitHub `production` release's `production.json`.
The production environment allows protected branches with no manual approval.
A stale CI run cannot promote over a newer main commit.

`everylotbot-deploy.timer` polls every five minutes. No GitHub SSH credential,
application secret, compiler, Docker image, or database is needed in CI/CD.
The repository and release assets are public; credentials remain on Lightsail.

## Persistent state

| State | Existing location / handling |
| --- | --- |
| Parcel database | `/home/ubuntu/bots/everylotbot-chicago/cook_county_lots.db`; never recreated, migrated, or replaced by deployment |
| Credentials | `/etc/everylotbot.env`; unchanged; historical repository `.env` also retained |
| Bluesky session | `/var/lib/everylotbot/bluesky-session.json`; normal worker refresh only |
| Historical logs | Existing journald records and bikeshare log files retained |
| New EveryLot output | Append to `/var/log/everylotbot/worker.log` and shared Petit task logs |
| Shared Petit history | Existing `/var/lib/petit-chicago-bikeshare-bot/history.sqlite3` |
| Application releases | `/opt/everylotbot/releases/<commit>/everylotbot`, atomic `current` symlink |
| Deployment backups | New files in `/var/lib/everylotbot/deploy-backups`; never automatically restored |

The deployer verifies checksums, waits for the posting lock, creates and checks
a consistent backup using a read-only SQLite source, and runs audit/dry-run on
a shadow under systemd with no network and read-only persistent mounts. It
checks that both shadow and live database bytes are unchanged before switching
the executable. Failed validation leaves the prior pointer intact. There is
no automatic migration, live post, credential write, log truncation, or backup
cleanup. Normal scheduled posts continue updating the original database.
Future schema changes require a separately reviewed migration procedure.

## Shared orchestration

`petit-bots.service` runs a pinned copy of the already installed Petit 0.2.0
(revision `170dee43be847b29bc065f4265a4b3e16d4fb7fd`) from `/opt/petit-bots/pt`.
It reads `/etc/petit-bots/jobs`, runs at most two jobs concurrently and one task
per job, and retains the existing history DB. This installed build has SQLite
support and no HTTP API; observability is local status, history, and journald.

EveryLot runs at minutes 0, 15, 30, 45 during UTC hours 0–7 and 12–23, matching
the previous systemd timer. Bikeshare keeps its existing six-hour schedule.
Both use independent locks and state. EveryLot clears the scheduler environment
and reads its own `.env` using `--env-file`, overriding only native file paths.

`chicago-bikeshare-bot.service.d/shared-petit.conf` is a persistent compatibility
drop-in: the old service becomes a oneshot bound to the real shared scheduler.
The bikeshare updater may replace its base unit and stop/start that name without
killing the shared scheduler or launching a duplicate. Its job YAML follows
`/opt/chicago-bikeshare-bot/current/deploy/jobs/bikeshare.yaml`.

Do not run a second Petit process against the same history DB. Drain both jobs
before restarting the shared scheduler. The existing bikeshare recovery hook
marks interrupted runs failed on startup, and its existing health routine retains
its established 90-day finished-run history policy. No new log deletion is added.

## Installation and configuration updates

The one-time migration installs `deploy/deploy-everylotbot` to
`/usr/local/sbin/deploy-everylotbot`, `deploy/run-everylotbot` to
`/usr/local/libexec/run-everylotbot`, and `deploy/petit-bots-status` to
`/usr/local/bin/petit-bots-status`. Install the deploy service/timer and shared
Petit service from this directory. Back up existing units first. Copy the pinned
Petit executable from the bike release into `/opt/petit-bots`, create the job
directory and compatibility drop-in, and preserve all state paths above.

For initial preparation keep EveryLot's Petit YAML disabled until the first
native artifact passes host validation. Then disable `everylotbot.timer`, install
the native `everylotbot.service` (manual recovery only), enable the Petit job,
and restart the drained shared scheduler. Never enable both posting schedules.
Future merges update application code only; host wrappers, units, or schedules
are reviewed and installed separately, not silently applied by the poller.

## Operations

```bash
sudo petit-bots-status
journalctl -u petit-bots.service -u everylotbot-deploy.service
sudo systemctl start everylotbot-deploy.service  # poll now, no live post
/opt/petit-bots/pt validate /etc/petit-bots/jobs
```

Use the `Roll back production` GitHub workflow with a full SHA of a previously
published native release. It promotes that immutable manifest; the host performs
the same validation before switching code. A manual `sudo deploy-everylotbot
--commit <SHA>` is useful for diagnosis but will be superseded by the next poll
unless the production manifest is also rolled back. Never restore an older
parcel DB over newer posting history as part of application rollback.

Historical Docker configuration and image are retained on the host for initial
emergency recovery only. Docker is not part of the new build or runtime.
