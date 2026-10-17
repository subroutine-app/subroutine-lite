#!/usr/bin/env python3

import fcntl
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import stat
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("deploy_fermi", Path(__file__).with_name("deploy-fermi.py"))
deploy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = deploy
spec.loader.exec_module(deploy)


class Runner:
    def __init__(self, case):
        self.case = case
        self.state = "active"
        self.commands = []
        self.on_start = lambda: None
        self.migrations = f"1,t,{hashlib.sha384(case.sources['crates/server/migrations/0001_init.sql']).hexdigest()}"

    def __call__(self, command, *, timeout=30):
        with self.case.lock.open("rb") as lock:
            with self.case.assertRaises(BlockingIOError):
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.commands.append(command)
        if command[0] == "runuser":
            self.case.assertEqual(command, [
                "runuser", "-u", "postgres", "--", "psql", "-X", "-v", "ON_ERROR_STOP=1",
                "-At", "-F", ",", "-d", "subroutine_lite_pilot", "-c", deploy.MIGRATION_QUERY,
            ])
            return self.migrations
        if command[0] == "restorecon":
            self.case.assertEqual(Path(command[1]).parent, self.case.paths.binary.parent)
            return ""
        self.case.assertEqual(command[0], "systemctl")
        self.case.assertEqual(command[-1], deploy.SERVICE)
        if command[1] == "show":
            return self.state if command[2] == "--property=ActiveState" else "1234"
        if command[1] == "stop":
            self.state = "inactive"
        elif command[1] == "start":
            self.on_start()
            self.state = "active"
        elif command[1] == "restart":
            self.state = "active"
        else:
            self.case.fail("unexpected command")
        return ""


class DeploymentTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.paths = deploy.Paths(self.root / "system")
        self.staging = self.root / "upload"
        self.owner = (os.geteuid(), os.getegid())
        for directory in (self.paths.binary.parent, self.paths.receipt,
                          self.paths.backups, self.paths.proc / "1234", self.staging):
            directory.mkdir(parents=True, mode=0o700)
        self.lock = self.paths.backups / "operation.lock"
        self.lock.write_bytes(b"")
        self.lock.chmod(0o600)
        self.destinations = (self.paths.binary, self.paths.receipt / "source.tar",
                             self.paths.receipt / "receipt.json")
        self.previous = (b"previous executable", b"previous source", b"previous receipt")
        for path, data in zip(self.destinations, self.previous):
            path.write_bytes(data)
            path.chmod(0o755 if path == self.paths.binary else 0o600)
        (self.paths.proc / "1234/exe").symlink_to(self.paths.binary)
        retained = self.paths.receipt / "deployments/older-good"
        retained.mkdir(parents=True, mode=0o700)
        (retained / "saved").write_bytes(b"retain this backup")
        unrelated = self.paths.receipt / "unrelated/nested"
        unrelated.mkdir(parents=True)
        (unrelated / "keep").write_bytes(b"retain unrelated receipt data")
        self.sources = {
            "Cargo.toml": b'[workspace]\nmembers = ["crates/server"]\n',
            "Cargo.lock": b"version = 4\n",
            "LICENSE": b"test license\n",
            "crates/server/migrations/0001_init.sql": b"CREATE TABLE example (id bigint);\n",
        }
        self.runner = Runner(self)
        self.upload()

    def upload(self, *, extra=None, omit=None):
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w") as tar:
            root = tarfile.TarInfo(".")
            root.type = tarfile.DIRTYPE
            tar.addfile(root)
            for name, data in self.sources.items():
                if name == omit:
                    continue
                entry = tarfile.TarInfo("./" + name)
                entry.size = len(data)
                tar.addfile(entry, io.BytesIO(data))
            if extra:
                tar.addfile(extra, io.BytesIO(b""))
        source = archive.getvalue()
        migration = self.sources["crates/server/migrations/0001_init.sql"]
        self.metadata = {
            "artifact_sha256": hashlib.sha256(b"new executable").hexdigest(),
            "source_archive_sha256": hashlib.sha256(source).hexdigest(),
            "source_files": {name: hashlib.sha256(data).hexdigest() for name, data in self.sources.items()},
            "migrations": {"0001_init.sql": {
                "version": 1,
                "sha256": hashlib.sha256(migration).hexdigest(),
                "checksum_sha384": hashlib.sha384(migration).hexdigest(),
            }},
        }
        (self.staging / "subroutine-lite-server").write_bytes(b"new executable")
        (self.staging / "source.tar").write_bytes(source)
        self.receipt()
        self.new = tuple((self.staging / name).read_bytes()
                         for name in ("subroutine-lite-server", "source.tar", "receipt.json"))

    def receipt(self):
        data = json.dumps(self.metadata).encode()
        (self.staging / "receipt.json").write_bytes(data)
        self.receipt_hash = hashlib.sha256(data).hexdigest()

    def deploy(self, **kwargs):
        return deploy.deploy(self.staging, self.receipt_hash, paths=self.paths,
                             runner=self.runner, probe=lambda timeout: True,
                             owner=self.owner, **kwargs)

    def snapshot(self):
        return {
            str(path.relative_to(self.paths.root)): (
                path.lstat().st_mode, path.lstat().st_ino,
                os.readlink(path) if path.is_symlink() else path.read_bytes() if path.is_file() else None,
            )
            for path in self.paths.root.rglob("*")
        }

    def assert_no_changes(self, action):
        before = self.snapshot()
        with self.assertRaises(deploy.DeployError):
            action()
        self.assertEqual(self.snapshot(), before)
        self.assertFalse(any(command[0] == "restorecon" or command[1] in ("stop", "start", "restart")
                             for command in self.runner.commands))

    def assert_preserved(self):
        self.assertEqual((self.paths.receipt / "unrelated/nested/keep").read_bytes(), b"retain unrelated receipt data")
        self.assertEqual((self.paths.receipt / "deployments/older-good/saved").read_bytes(), b"retain this backup")
        self.assertFalse(list(self.paths.root.rglob(".deploy-*")))

    def assert_backup(self, backup):
        self.assertEqual(stat.S_IMODE(backup.stat().st_mode), 0o700)
        for path, data in zip(self.destinations, self.previous):
            saved = backup / path.name
            self.assertEqual(saved.read_bytes(), data)
            self.assertEqual(stat.S_IMODE(saved.stat().st_mode), 0o600)
        self.assertEqual((backup / "new-receipt.json").read_bytes(), self.new[2])

    def test_success_installs_verified_files_and_keeps_backups(self):
        inode = self.lock.stat().st_ino
        report = self.deploy()
        self.assertEqual(report["status"], "ok")
        self.assertEqual(report["artifact_sha256"], self.metadata["artifact_sha256"])
        self.assertEqual(report["receipt_sha256"], self.receipt_hash)
        self.assertEqual(tuple(path.read_bytes() for path in self.destinations), self.new)
        for path in self.destinations:
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o755 if path == self.paths.binary else 0o600)
            self.assertEqual((path.stat().st_uid, path.stat().st_gid), self.owner)
        self.assertEqual(self.lock.stat().st_ino, inode)
        self.assert_backup(Path(report["rollback_path"]))
        self.assert_preserved()

    def test_failed_start_restores_all_three_files_and_verifies_recovery(self):
        def fail():
            self.assertEqual(tuple(path.read_bytes() for path in self.destinations), self.new)
            raise deploy.DeployError("systemctl failed")

        self.runner.on_start = fail
        with self.assertRaises(deploy.DeployError) as caught:
            self.deploy()
        self.assertEqual(caught.exception.rollback, "verified")
        self.assertEqual(tuple(path.read_bytes() for path in self.destinations), self.previous)
        self.assertEqual(self.runner.state, "active")
        self.assertEqual([cmd[1] for cmd in self.runner.commands if cmd[0] == "systemctl" and cmd[1] != "show"],
                         ["stop", "start", "stop", "restart"])
        self.assert_backup(Path(caught.exception.rollback_path))
        self.assert_preserved()

    def test_partial_swap_restores_all_three_files(self):
        replace = os.replace
        replacements = 0

        def fail_second_swap(source, destination):
            nonlocal replacements
            replacements += 1
            self.assertEqual(self.runner.state, "inactive")
            if replacements == 2:
                self.assertEqual(self.paths.binary.read_bytes(), self.new[0])
                raise OSError("simulated filesystem failure")
            replace(source, destination)

        with patch.object(deploy.os, "replace", side_effect=fail_second_swap):
            with self.assertRaises(deploy.DeployError) as caught:
                self.deploy()
        self.assertEqual(caught.exception.rollback, "verified")
        self.assertEqual(tuple(path.read_bytes() for path in self.destinations), self.previous)
        self.assert_backup(Path(caught.exception.rollback_path))
        self.assert_preserved()

    def test_mismatched_uploads_abort_without_writes(self):
        for name in ("subroutine-lite-server", "source.tar", "receipt.json"):
            with self.subTest(name=name):
                self.upload()
                (self.staging / name).write_bytes(b"tampered")
                self.assert_no_changes(self.deploy)

    def test_unsafe_or_incomplete_archives_abort_without_writes(self):
        for name, kind in (("../escape", tarfile.REGTYPE), ("/absolute", tarfile.REGTYPE),
                           ("crates/../../escape", tarfile.DIRTYPE), ("link", tarfile.SYMTYPE),
                           ("hardlink", tarfile.LNKTYPE), ("pipe", tarfile.FIFOTYPE),
                           ("extra", tarfile.REGTYPE), ("Cargo.toml", tarfile.REGTYPE),
                           ("Cargo.lock", tarfile.DIRTYPE)):
            with self.subTest(name=name, kind=kind):
                entry = tarfile.TarInfo(name)
                entry.type = kind
                entry.linkname = "Cargo.toml"
                self.upload(extra=entry)
                self.assert_no_changes(self.deploy)
        self.upload(omit="LICENSE")
        self.assert_no_changes(self.deploy)
        self.assertFalse((self.root / "escape").exists())

    def test_source_and_migration_receipts_must_match_archive_and_database(self):
        for change in (
            lambda: self.metadata["source_files"].update({"Cargo.toml": "0" * 64}),
            lambda: self.metadata["migrations"]["0001_init.sql"].update(checksum_sha384="0" * 96),
            lambda: self.metadata["migrations"]["0001_init.sql"].update(version=2),
        ):
            with self.subTest(change=change):
                self.upload()
                change()
                self.receipt()
                self.assert_no_changes(self.deploy)
        self.upload()
        self.runner.migrations = "1,t," + "0" * 96
        self.assert_no_changes(self.deploy)

    def test_operation_guards_abort_without_writes(self):
        with self.lock.open("rb") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assert_no_changes(self.deploy)
        pending = self.paths.backups / "pending-restart.json"
        pending.write_bytes(b"pending")
        self.assert_no_changes(self.deploy)
        pending.unlink()
        self.runner.state = "inactive"
        self.assert_no_changes(self.deploy)

    def test_normal_signals_roll_back_and_release_the_lock(self):
        for number in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signal=number):
                self.runner.on_start = lambda: signal.raise_signal(number)
                with deploy.signals(deploy.interrupted):
                    with self.assertRaises(deploy.DeployError) as caught:
                        self.deploy()
                self.assertEqual(caught.exception.rollback, "verified")
                self.assertEqual(tuple(path.read_bytes() for path in self.destinations), self.previous)
                with self.lock.open("rb") as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                self.assert_backup(Path(caught.exception.rollback_path))

    def test_wrong_running_executable_rolls_back(self):
        executable = self.paths.proc / "1234/exe"
        stale = self.root / "wrong-executable"
        stale.write_bytes(b"wrong executable")

        def wrong_process():
            executable.unlink()
            executable.symlink_to(stale)

        original_runner = self.runner

        def runner(command, **kwargs):
            if command[:2] == ["systemctl", "restart"]:
                executable.unlink()
                executable.symlink_to(self.paths.binary)
            return original_runner(command, **kwargs)

        self.runner.on_start = wrong_process
        self.runner = runner
        with self.assertRaises(deploy.DeployError) as caught:
            self.deploy()
        self.assertEqual(caught.exception.rollback, "verified")
        self.assertIn("running executable checksum mismatch", str(caught.exception))
        self.assertEqual(tuple(path.read_bytes() for path in self.destinations), self.previous)

    def test_command_errors_do_not_include_output_or_arguments(self):
        result = type("Result", (), {"returncode": 1, "stdout": b"secret stdout", "stderr": b"secret stderr"})()
        with patch.object(deploy.subprocess, "run", return_value=result):
            with self.assertRaisesRegex(deploy.DeployError, "^systemctl failed$"):
                deploy.run(["systemctl", "private argument"])


if __name__ == "__main__":
    unittest.main()
