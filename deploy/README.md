# Lightsail migration runbook

This runbook migrates only EveryLot Chicago. It must not stop or reconfigure the other PM2 bots on the host.

## Fixed production facts

- Host: Ubuntu 24.04 x86_64, 416 MiB RAM.
- Live database: `/home/ubuntu/bots/everylotbot-chicago/cook_county_lots.db`.
- Existing scheduler: PM2, 80 invocations per day.
- Posting gap: `08:00` through `11:59` UTC.
- Authoritative legacy schedule: `*/15 0,1,2,3,4,5,6,7,12,13,14,15,16,17,18,19,20,21,22,23 * * *`.
- The replacement systemd timer expresses the same schedule in UTC.

Never derive the rollback schedule from the older untracked production `ecosystem.config.js`; it differs from PM2's loaded schedule. Use `legacy-everylotbot.pm2.cjs` from this directory.

## Before the maintenance window

1. Obtain the CI-built Linux x86_64 artifact and verify its SHA-256 checksum.
2. Install the latest patched Node.js 24 release at `/opt/node-v24` without modifying the NVM-managed Node 23 used by PM2.
3. Extract the release under `/opt/everylotbot/releases/<git-sha>` and point `/opt/everylotbot/current` to it.
4. Create `/var/lib/everylotbot`, owned by `ubuntu:ubuntu`, mode `0700`.
5. Create `/etc/everylotbot.env`, owned by `root:ubuntu`, mode `0640`, using `everylotbot.env.example` and the existing production secrets.
6. Copy the service and timer into `/etc/systemd/system`, run `systemctl daemon-reload`, but do not enable or start the timer.
7. Validate the artifact:

   ```bash
   /opt/node-v24/bin/node --version
   cd /opt/everylotbot/current
   sudo -u ubuntu /opt/node-v24/bin/node dist/src/cli/audit.js
   sudo -u ubuntu /opt/node-v24/bin/node dist/src/cli/post-next.js --dry-run
   systemd-analyze calendar '*-*-* 00..07,12..23:00/15:00 UTC'
   ```

Audit and dry-run open SQLite read-only and must not create migration tables, fetch Street View, authenticate, or post.

## Cutover: perform between 08:00 and 11:59 UTC

Record values in a timestamped directory under `/home/ubuntu/migration-backups`.

1. Verify that the 07:45 run exited and no EveryLot process exists.
2. Capture `pm2 describe everylotbot-chicago`, `pm2 jlist` with environment values removed, `git status`, `git rev-parse HEAD`, the loaded cron expression, disk usage, and the current database audit.
3. Create a manual Lightsail instance snapshot and wait for it to complete.
4. Create a consistent SQLite backup:

   ```bash
   sqlite3 /home/ubuntu/bots/everylotbot-chicago/cook_county_lots.db \
     ".backup '/home/ubuntu/migration-backups/everylot-<timestamp>/cook_county_lots.db'"
   sqlite3 /home/ubuntu/migration-backups/everylot-<timestamp>/cook_county_lots.db 'PRAGMA integrity_check;'
   ```

5. Record the live last and next PIN10. The values discovered in July 2026 are historical and must not be reused.
6. Remove only this PM2 app and persist the PM2 process list:

   ```bash
   pm2 delete everylotbot-chicago
   pm2 save
   ```

7. Run the TypeScript audit and dry run again. Both must report the same next PIN10 captured in step 5.
8. Start the service once with a temporary no-post mechanism only if needed; otherwise let the normal noon boundary be the first write. Enabling the timer is the first action that authorizes posting:

   ```bash
   sudo systemctl enable --now everylotbot.timer
   systemctl list-timers everylotbot.timer --no-pager
   ```

9. Verify that the next trigger is `12:00 UTC`. The timer deliberately uses `Persistent=false`, matching PM2 behavior: missed social posts are not replayed as a burst when the host returns.

The first real invocation applies additive tables, takes a database lease, and posts one lot. It does not alter the legacy table schema.

## First-run checks

After the scheduled run:

```bash
systemctl status everylotbot.service --no-pager
journalctl -u everylotbot.service --since '15 minutes ago' --no-pager
sudo -u ubuntu /opt/node-v24/bin/node /opt/everylotbot/current/dist/src/cli/audit.js
sqlite3 /home/ubuntu/bots/everylotbot-chicago/cook_county_lots.db 'PRAGMA integrity_check;'
```

Verify one public Bluesky post, one new URL in `lots.posted_bluesky`, and a one-row cursor advance.

## Rollback

Disable the TypeScript scheduler first:

```bash
sudo systemctl disable --now everylotbot.timer
sudo systemctl stop everylotbot.service
```

If no delivery is uncertain, restore Python using the captured schedule:

```bash
cd /home/ubuntu/bots/everylotbot-chicago
pm2 start /opt/everylotbot/current/deploy/legacy-everylotbot.pm2.cjs --only everylotbot-chicago
pm2 save
```

Successful TypeScript posts also update `lots.posted_bluesky`, so Python continues at the next lot without restoring the database. If a delivery is `unknown`, leave both schedulers disabled and reconcile the deterministic Bluesky record before restarting either scheduler. Restore the verified SQLite backup only for database corruption; preserve the damaged file for diagnosis.

## Observation and cleanup

Check every run for the first two hours, all 80 runs during day one, and daily totals for seven days. Do not delete Python, its venv, the PM2 rollback file, or the pre-cutover backup during that period.
