#!/usr/bin/env bash
set -euo pipefail
binary=$(realpath "${1:-target/release/everylotbot}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
sqlite3 "$work/parcels.db" < schema.sql
sqlite3 "$work/parcels.db" < src/schema-v1.sql
sqlite3 "$work/parcels.db" "INSERT INTO lots(id,address) VALUES ('0123456789','1 N TEST ST');"
before=$(sha256sum "$work/parcels.db" | cut -d' ' -f1)
(cd "$work"; env -i PATH="$PATH" "$binary" audit --database "$work/parcels.db")
(cd "$work"; env -i PATH="$PATH" "$binary" post-next --database "$work/parcels.db" --dry-run)
[[ $(sha256sum "$work/parcels.db" | cut -d' ' -f1) == "$before" ]]
if "$binary" audit --database "$work/missing.db"; then exit 1; fi
[[ ! -e "$work/missing.db" ]]
