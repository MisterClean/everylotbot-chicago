# Production deployment

Production uses a pull-based deployment. GitHub Actions builds and attests one
`linux/amd64` image after `CI / test` succeeds on `main`, then promotes that
exact digest to the GHCR `production` tag. The Lightsail host polls the tag and
runs the image by digest.

The deployment boundary is intentionally strict:

- application code is contained in the immutable image;
- `/etc/everylotbot.env` remains only on the host;
- the live SQLite database remains at
  `/home/ubuntu/bots/everylotbot-chicago/cook_county_lots.db`;
- the Bluesky session remains under `/var/lib/everylotbot`;
- candidate migrations, audits, and dry runs use a disposable shadow database;
- deployment and rollback change only the image pointer, never the live database.

## GitHub prerequisites

1. Create a `production` environment, restrict it to protected branches, and
   require approval for the initial rollouts.
2. Protect `main` with pull requests, required `CI / test`, resolved review
   conversations, and blocks on force pushes and deletion.
3. Make `ghcr.io/misterclean/everylotbot-chicago` public after its first publish
   so the host can pull it without a long-lived GitHub credential.

GitHub does not need a production SSH key, application secrets, or database.

## Host installation

Keep the deployment timer disabled until manual deployment and rollback checks
have passed:

```bash
sudo install -o root -g root -m 0755 \
  deploy/deploy-everylotbot /usr/local/sbin/deploy-everylotbot
sudo install -o root -g root -m 0644 \
  deploy/everylotbot-deploy.service /etc/systemd/system/everylotbot-deploy.service
sudo install -o root -g root -m 0644 \
  deploy/everylotbot-deploy.timer /etc/systemd/system/everylotbot-deploy.timer
sudo systemctl daemon-reload
```

Manually validate and select a known published digest:

```bash
sudo deploy-everylotbot --digest sha256:<known-digest>
```

After the pointer is written, install the container-backed application units:

```bash
sudo install -o root -g root -m 0644 \
  deploy/everylotbot.service /etc/systemd/system/everylotbot.service
sudo install -o root -g root -m 0644 \
  deploy/everylotbot.timer /etc/systemd/system/everylotbot.timer
sudo systemctl daemon-reload
sudo systemctl enable --now everylotbot.timer
```

After manual deployment and rollback checks succeed:

```bash
sudo systemctl enable --now everylotbot-deploy.timer
```

The deployer:

- refuses to pull with less than 2 GiB free;
- serializes deployment with a host lock;
- defers if a production posting cycle is active;
- creates and integrity-checks a consistent SQLite backup;
- applies candidate migrations only to a disposable shadow copy;
- rejects removed or incompatibly changed tables, indexes, and columns;
- runs candidate audit and dry-run commands without network access;
- removes the disposable shadow after successful validation;
- atomically pins `/etc/everylotbot-image.env` to the exact digest;
- preserves the database backup and deployment metadata;
- keeps the current and previous EveryLot images;
- restores the regular posting timer after success, failure, or interruption.

Logs are available with:

```bash
journalctl -u everylotbot-deploy.service
journalctl -u everylotbot.service
```

## Rollback

Run the `Roll back production` GitHub workflow with a full commit SHA from
`main` or a previously published `sha256:` digest. It validates the image and
moves the GHCR `production` tag to that exact digest. The host poller performs
the same backup and shadow validation before changing the image pointer.

Never restore a database automatically after an application rollback. Posting
state may have advanced since the older code was released. Restore a saved
database only during an explicitly approved corruption-recovery procedure.
