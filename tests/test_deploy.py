"""Run the real deploy script on isolated fixtures; fake transport/systemd only.

Linux CI supplies flock and GNU filesystem tools. No root, network, production
paths, credentials, or live posting are involved.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(sys.platform == "linux", "Linux host deployment contract")
class DeploymentTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = Path(__file__).resolve().parents[1]
        self.state = self.root / "state"
        self.state.mkdir()
        self.application = self.root / "app"
        self.database = self.root / "parcels.db"
        self.credentials = self.root / "production.env"
        self.credentials.write_text("KEEP_THIS=unchanged\n", encoding="utf-8")
        self.log = self.root / "worker.log"
        self.log.write_text("historical logs\n", encoding="utf-8")
        with sqlite3.connect(self.database) as db:
            db.executescript((self.repo / "schema.sql").read_text(encoding="utf-8"))
            db.executescript((self.repo / "src/schema-v1.sql").read_text(encoding="utf-8"))
            db.execute("INSERT INTO lots(id,address) VALUES ('0123456789','1 N TEST ST')")
        self.original = self.database.read_bytes()
        self.transport = self.root / "transport"
        self.transport.mkdir()
        self.mocks = self.root / "bin"
        self.mocks.mkdir()
        self.script = self.root / "deploy"
        script = (self.repo / "deploy/deploy-everylotbot").read_text(encoding="utf-8")
        for source, replacement in {
            "/opt/everylotbot": str(self.application),
            "/var/lib/everylotbot": str(self.state),
            "/home/ubuntu/bots/everylotbot-chicago/cook_county_lots.db": str(self.database),
            "/etc/everylotbot.env": str(self.credentials),
            "/run/lock/everylotbot-deploy.lock": str(self.root / "deploy.lock"),
            "[[ $EUID == 0 ]]": "true",
        }.items():
            script = script.replace(source, replacement)
        self.script.write_text(script, encoding="utf-8")
        self.mock("curl", '''output=''
while (($#)); do
  case "$1" in
    --output) output=$2; shift 2;;
    https://*) url=$1; shift;;
    *) shift;;
  esac
done
asset=${url##*/}
if [[ ! -e "$TRANSPORT/$asset" ]]; then printf 404; exit 0; fi
cp "$TRANSPORT/$asset" "$output"
[[ $asset != production.json ]] || printf 200
''')
        self.mock("systemd-run", '''[[ " $* " == *" --property=PrivateNetwork=yes "* ]]
[[ " $* " == *" --property=ProtectSystem=strict "* ]]
while [[ $1 == --* ]]; do shift; done
[[ ${FAIL_VALIDATION:-0} == 0 ]] || exit 42
exec "$@"
''')
        self.mock("chown", "exit 0\n")
        self.mock("install", '''args=()
while (($#)); do
  case "$1" in -o|-g) shift 2;; *) args+=("$1"); shift;; esac
done
exec /usr/bin/install "${args[@]}"
''')
        self.env = {**os.environ, "PATH": f"{self.mocks}:{os.environ['PATH']}",
                    "TRANSPORT": str(self.transport)}

    def mock(self, name: str, script: str) -> None:
        path = self.mocks / name
        path.write_text("#!/usr/bin/env bash\nset -euo pipefail\n" + script, encoding="utf-8")
        path.chmod(0o755)

    def publish(self, commit: str) -> None:
        binary = self.transport / "everylotbot-linux-x86_64"
        shutil.copyfile(self.repo / "target/release/everylotbot", binary)
        manifest = {"schema": 1, "platform": "linux-x86_64", "commit": commit,
                    "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
        (self.transport / "production.json").write_text(json.dumps(manifest), encoding="utf-8")

    def deploy(self, *args: str, success: bool = True) -> None:
        result = subprocess.run(["bash", str(self.script), *args], env=self.env,
                                capture_output=True, text=True, check=False, timeout=30)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.database.read_bytes(), self.original)
        self.assertEqual(self.credentials.read_text(encoding="utf-8"), "KEEP_THIS=unchanged\n")
        self.assertEqual(self.log.read_text(encoding="utf-8"), "historical logs\n")

    def test_deploy_idempotence_and_code_only_rollback(self) -> None:
        first, second = "a" * 40, "b" * 40
        self.publish(first)
        self.deploy()
        first_pointer = (self.application / "current").readlink()
        self.deploy()
        self.assertEqual(len(list((self.state / "deploy-backups").iterdir())), 1)
        self.publish(second)
        self.deploy()
        self.assertNotEqual((self.application / "current").readlink(), first_pointer)
        self.publish(first)
        self.deploy("--commit", first)
        self.assertEqual((self.application / "current").readlink(), first_pointer)

    def test_checksum_failure_keeps_current_executable(self) -> None:
        self.publish("a" * 40)
        self.deploy()
        pointer = (self.application / "current").readlink()
        self.publish("b" * 40)
        (self.transport / "everylotbot-linux-x86_64").write_bytes(b"corrupted")
        self.deploy(success=False)
        self.assertEqual((self.application / "current").readlink(), pointer)

    def test_shadow_validation_failure_never_promotes(self) -> None:
        self.publish("a" * 40)
        self.env["FAIL_VALIDATION"] = "1"
        self.deploy(success=False)
        self.assertFalse((self.application / "current").is_symlink())

    def test_unpublished_release_is_safe_during_preparation(self) -> None:
        self.deploy()
        self.assertFalse((self.application / "current").is_symlink())

    def test_manifest_commit_cannot_escape_release_directory(self) -> None:
        self.publish("../../elsewhere")
        self.deploy(success=False)
        self.assertFalse((self.application / "current").is_symlink())

    def test_requested_rollback_must_match_manifest(self) -> None:
        self.publish("a" * 40)
        self.deploy("--commit", "b" * 40, success=False)
        self.assertFalse((self.application / "current").is_symlink())
