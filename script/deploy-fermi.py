#!/usr/bin/env python3
"""Deploy the Lite pilot as root. SIGKILL/power loss is not crash-atomic."""

import contextlib
import dataclasses
import fcntl
import hashlib
import http.client
import io
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time


SERVICE = "subroutine-lite-pilot.service"
MIGRATIONS = "crates/server/migrations/"
MIGRATION_QUERY = (
    "SELECT version,success,encode(checksum,'hex') "
    "FROM _sqlx_migrations ORDER BY version;"
)


class DeployError(Exception):
    def __init__(self, message, *, rollback_path=None, rollback=None):
        super().__init__(message)
        self.rollback_path = rollback_path
        self.rollback = rollback


@dataclasses.dataclass(frozen=True)
class Paths:
    root: Path = Path("/")

    @property
    def binary(self):
        return self.root / "usr/local/bin/subroutine-lite-pilot"

    @property
    def receipt(self):
        return self.root / "etc/subroutine-lite-pilot/receipt"

    @property
    def backups(self):
        return self.root / "var/backups/subroutine-lite-pilot"

    @property
    def proc(self):
        return self.root / "proc"


@dataclasses.dataclass(frozen=True)
class File:
    path: Path
    data: bytes
    mode: int

    @property
    def sha256(self):
        return hashlib.sha256(self.data).hexdigest()


def digest(value, length=64):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{%d}" % length, value):
        raise DeployError("invalid receipt checksum")
    return value


def regular_file(path, *, dir_fd=None):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=dir_fd)
    with os.fdopen(fd, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise DeployError("expected a regular file")
        return stream.read()


def json_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise DeployError("duplicate receipt key")
        result[key] = value
    return result


def relative_path(name, *, directory=False):
    if not isinstance(name, str) or not name or name.startswith("/") or "\\" in name or "\0" in name:
        raise DeployError("unsafe source path")
    parts = name.split("/")
    if ".." in parts:
        raise DeployError("unsafe source path")
    normalized = "/".join(part for part in parts if part not in ("", "."))
    if not normalized and not directory:
        raise DeployError("unsafe source path")
    return normalized


def validate_archive(source, source_files):
    if not isinstance(source_files, dict) or not {"Cargo.toml", "Cargo.lock", "LICENSE"} <= source_files.keys():
        raise DeployError("required source files missing")
    for name, checksum in source_files.items():
        if relative_path(name) != name:
            raise DeployError("noncanonical source path")
        digest(checksum)
        if any(str(parent) in source_files for parent in Path(name).parents):
            raise DeployError("source file and directory conflict")

    seen = set()
    files = set()
    migrations = {}
    with tarfile.open(fileobj=io.BytesIO(source), mode="r:") as archive:
        for entry in archive:
            name = relative_path(entry.name, directory=entry.isdir())
            if name in seen:
                raise DeployError("duplicate source archive entry")
            seen.add(name)
            if entry.isdir():
                if name in source_files or any(str(parent) in source_files for parent in Path(name).parents):
                    raise DeployError("source file and directory conflict")
                continue
            if not entry.isreg() or entry.issparse() or name not in source_files:
                raise DeployError("unexpected or unsafe source archive entry")
            with archive.extractfile(entry) as stream:
                data = stream.read()
            if hashlib.sha256(data).hexdigest() != source_files[name]:
                raise DeployError("source file checksum mismatch")
            files.add(name)
            if name.startswith(MIGRATIONS) and "/" not in name[len(MIGRATIONS):] and name.endswith(".sql"):
                migrations[name[len(MIGRATIONS):]] = (
                    source_files[name], hashlib.sha384(data).hexdigest()
                )
    if files != source_files.keys() or not migrations:
        raise DeployError("source archive is incomplete")
    return migrations


def validate_migrations(metadata, sources):
    if not isinstance(metadata, dict) or metadata.keys() != sources.keys():
        raise DeployError("migration receipt does not match source files")
    expected = {}
    for name, entry in metadata.items():
        if not isinstance(entry, dict):
            raise DeployError("invalid migration receipt")
        version = entry.get("version")
        prefix, separator, _ = name.partition("_")
        if (type(version) is not int or not 0 < version < 2**63
                or not separator or not prefix.isascii() or not prefix.isdigit()
                or int(prefix) != version):
            raise DeployError("invalid migration version")
        checksums = (digest(entry.get("sha256")), digest(entry.get("checksum_sha384"), 96))
        if checksums != sources[name]:
            raise DeployError("migration checksum mismatch")
        if not name.endswith(".down.sql"):
            if version in expected:
                raise DeployError("duplicate migration version")
            expected[version] = checksums[1]
    if not expected:
        raise DeployError("no forward migrations in receipt")
    return tuple(f"{version},t,{checksum}" for version, checksum in sorted(expected.items()))


def validate_upload(staging, expected_receipt, paths):
    digest(expected_receipt)
    fd = os.open(staging, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        receipt = regular_file("receipt.json", dir_fd=fd)
        if hashlib.sha256(receipt).hexdigest() != expected_receipt:
            raise DeployError("receipt checksum mismatch")
        metadata = json.loads(receipt, object_pairs_hook=json_object)
        if not isinstance(metadata, dict):
            raise DeployError("invalid receipt")
        artifact_hash = digest(metadata.get("artifact_sha256"))
        source_hash = digest(metadata.get("source_archive_sha256"))
        artifact = regular_file("subroutine-lite-server", dir_fd=fd)
        source = regular_file("source.tar", dir_fd=fd)
    finally:
        os.close(fd)
    if hashlib.sha256(artifact).hexdigest() != artifact_hash:
        raise DeployError("artifact checksum mismatch")
    if hashlib.sha256(source).hexdigest() != source_hash:
        raise DeployError("source archive checksum mismatch")
    sources = validate_archive(source, metadata.get("source_files"))
    migrations = validate_migrations(metadata.get("migrations"), sources)
    # Keep the verified bytes, not mutable paths into the uploader's staging directory.
    return (
        File(paths.binary, artifact, 0o755),
        File(paths.receipt / "source.tar", source, 0o600),
        File(paths.receipt / "receipt.json", receipt, 0o600),
    ), migrations


def run(command, *, timeout=30):
    try:
        result = subprocess.run(
            command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, timeout=timeout, check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        raise DeployError(f"{command[0]} failed") from None
    if result.returncode:
        raise DeployError(f"{command[0]} failed")
    return result.stdout.decode("utf-8", errors="replace").strip()


def active(runner, *, timeout=30):
    return runner(["systemctl", "show", "--property=ActiveState", "--value", SERVICE], timeout=timeout) == "active"


def check_migrations(expected, runner):
    result = runner([
        "runuser", "-u", "postgres", "--", "psql", "-X", "-v", "ON_ERROR_STOP=1",
        "-At", "-F", ",", "-d", "subroutine_lite_pilot", "-c", MIGRATION_QUERY,
    ])
    if tuple(result.splitlines()) != expected:
        raise DeployError("live migration history differs; no database changes were made")


def private_directory(path, owner):
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != owner[0] or info.st_mode & 0o022:
        raise DeployError("deployment directory is not securely owned")


@contextlib.contextmanager
def operation_lock(paths, owner):
    private_directory(paths.backups, owner)
    fd = os.open(paths.backups / "operation.lock", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != owner[0] or info.st_mode & 0o022:
            raise DeployError("operation lock is not securely owned")
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise DeployError("another Lite operation holds the lock") from None
        yield
    finally:
        os.close(fd)


def save_backup(previous, receipt, paths, owner):
    parent = paths.receipt / "deployments"
    parent.mkdir(mode=0o700, exist_ok=True)
    private_directory(parent, owner)
    backup = Path(tempfile.mkdtemp(prefix="deploy-", dir=parent))
    os.chown(backup, *owner)
    for file in (*previous, File(Path("new-receipt.json"), receipt, 0o600)):
        with (backup / file.path.name).open("xb") as stream:
            os.fchown(stream.fileno(), *owner)
            os.fchmod(stream.fileno(), 0o600)
            stream.write(file.data)
            stream.flush()
            os.fsync(stream.fileno())
    return backup


@contextlib.contextmanager
def staged_files(files, paths, owner, runner):
    with contextlib.ExitStack() as cleanup:
        staged = []
        for file in files:
            fd, name = tempfile.mkstemp(prefix=".deploy-", dir=file.path.parent)
            temporary = Path(name)
            cleanup.callback(temporary.unlink, missing_ok=True)
            with os.fdopen(fd, "wb") as stream:
                os.fchown(stream.fileno(), *owner)
                os.fchmod(stream.fileno(), file.mode)
                stream.write(file.data)
                stream.flush()
                os.fsync(stream.fileno())
            if file.path == paths.binary:
                runner(["restorecon", str(temporary)])
            staged.append((temporary, file.path))
        yield staged


def http_unauthorized(timeout):
    connection = http.client.HTTPConnection("127.0.0.1", 3300, timeout=timeout)
    try:
        connection.request("GET", "/v1/account")
        return connection.getresponse().status == 401
    except (OSError, http.client.HTTPException):
        return False
    finally:
        connection.close()


def verify_files(files):
    for file in files:
        if hashlib.sha256(regular_file(file.path)).hexdigest() != file.sha256:
            raise DeployError("installed file checksum mismatch")


def stream_sha256(stream):
    checksum = hashlib.sha256()
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        checksum.update(block)
    return checksum.hexdigest()


def wait_healthy(files, paths, runner, probe, *, timeout=30):
    deadline = time.monotonic() + timeout

    def remaining():
        return max(0.001, min(2, deadline - time.monotonic()))

    while time.monotonic() < deadline:
        if active(runner, timeout=remaining()) and probe(remaining()):
            command = ["systemctl", "show", "--property=MainPID", "--value", SERVICE]
            pid = runner(command, timeout=remaining())
            if not re.fullmatch(r"[1-9][0-9]*", pid):
                raise DeployError("Lite service has no main process")
            with (paths.proc / pid / "exe").open("rb") as executable:
                checksum = stream_sha256(executable)
            if checksum != files[0].sha256:
                raise DeployError("running executable checksum mismatch")
            verify_files(files)
            if active(runner, timeout=remaining()) and runner(command, timeout=remaining()) == pid:
                return
        time.sleep(max(0, min(0.25, deadline - time.monotonic())))
    raise DeployError("Lite service health check failed")


def interrupted(signum, frame):
    raise DeployError("deployment interrupted")


@contextlib.contextmanager
def signals(handler):
    previous = {}
    try:
        for number in (signal.SIGTERM, signal.SIGINT):
            previous[number] = signal.signal(number, handler)
        yield
    finally:
        for number, original in previous.items():
            signal.signal(number, original)


def safe_error(error):
    if isinstance(error, DeployError):
        return str(error)
    if isinstance(error, KeyboardInterrupt):
        return "deployment interrupted"
    return f"deployment failed ({type(error).__name__})"


def rollback(previous, paths, owner, runner, probe):
    errors = []

    def attempt(action):
        try:
            action()
        except Exception as error:
            errors.append(safe_error(error))

    def restore(file):
        with staged_files((file,), paths, owner, runner) as staged:
            os.replace(*staged[0])

    # Once recovery begins, repeated terminal signals must not interrupt restoration.
    with signals(signal.SIG_IGN):
        attempt(lambda: runner(["systemctl", "stop", SERVICE]))
        for file in previous:
            attempt(lambda file=file: restore(file))
        attempt(lambda: runner(["restorecon", str(paths.binary)]))
        attempt(lambda: runner(["systemctl", "restart", SERVICE]))
        attempt(lambda: verify_files(previous))
        attempt(lambda: wait_healthy(previous, paths, runner, probe))
    if errors:
        raise DeployError("rollback incomplete: " + "; ".join(dict.fromkeys(errors)))


def deploy(staging, expected_receipt, *, paths=Paths(), runner=run,
           probe=http_unauthorized, owner=(0, 0)):
    with operation_lock(paths, owner):
        if os.path.lexists(paths.backups / "pending-restart.json"):
            raise DeployError("pending restart requires recovery before deployment")
        if not active(runner):
            raise DeployError("Lite service is not active")
        files, migrations = validate_upload(staging, expected_receipt, paths)
        check_migrations(migrations, runner)
        private_directory(paths.binary.parent, owner)
        private_directory(paths.receipt, owner)
        previous = tuple(File(file.path, regular_file(file.path), file.mode) for file in files)
        backup = save_backup(previous, files[2].data, paths, owner)
        touched = False
        try:
            with staged_files(files, paths, owner, runner) as staged:
                touched = True
                runner(["systemctl", "stop", SERVICE])
                for temporary, destination in staged:
                    os.replace(temporary, destination)
                runner(["restorecon", str(paths.binary)])
                runner(["systemctl", "start", SERVICE])
                wait_healthy(files, paths, runner, probe)
        except BaseException as error:
            if touched:
                try:
                    rollback(previous, paths, owner, runner, probe)
                except BaseException as recovery_error:
                    raise DeployError(
                        safe_error(error) + "; " + safe_error(recovery_error),
                        rollback_path=str(backup), rollback="failed",
                    ) from None
            raise DeployError(
                safe_error(error), rollback_path=str(backup),
                rollback="verified" if touched else "not-needed",
            ) from None
        return {"status": "ok", "artifact_sha256": files[0].sha256,
                "receipt_sha256": expected_receipt, "rollback_path": str(backup)}


def main(argv=None):
    try:
        args = sys.argv[1:] if argv is None else argv
        if len(args) != 2:
            raise DeployError("usage: deploy-fermi.py STAGING_DIR EXPECTED_RECEIPT_SHA256")
        if os.geteuid() != 0:
            raise DeployError("deployment requires root")
        with signals(interrupted):
            result = deploy(Path(args[0]), args[1])
    except BaseException as error:
        result = {"status": "error", "error": safe_error(error)}
        if isinstance(error, DeployError) and error.rollback_path:
            result.update(rollback_path=error.rollback_path, rollback=error.rollback)
        print(json.dumps(result, separators=(",", ":")))
        return 1
    print(json.dumps(result, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
