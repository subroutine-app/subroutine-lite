
import argparse
import os
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path

from licenses import write_licenses

APP_ID = "com.subroutine.SubroutineLite"
ROOT = Path(__file__).resolve().parents[4]
RESOURCES = ROOT / "crates/desktop/resources"


def public_metadata(info):
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    info.mtime = 0
    info.pax_headers = {}
    return info


def package(build, target, release):
    executable = build / "subroutine-lite"
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise SystemExit(f"Expected executable was not produced: {executable}")
    output = build / "bundle/linux"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"subroutine-lite-{target}.tar.gz"
    with tempfile.TemporaryDirectory(prefix="subroutine-lite-linux-") as directory:
        staging = Path(directory)
        bundle = staging / "subroutine-lite"
        (bundle / "bin").mkdir(parents=True)
        shutil.copyfile(executable, bundle / "bin/subroutine-lite")
        (bundle / "bin/subroutine-lite").chmod(0o755)
        icons = RESOURCES / "linux/hicolor"
        for size in (16, 24, 32, 48, 64, 128, 256, 512, 1024, "scalable"):
            group = "scalable" if size == "scalable" else f"{size}x{size}"
            extension = "svg" if size == "scalable" else "png"
            relative = Path(group) / "apps" / f"{APP_ID}.{extension}"
            destination = bundle / "share/icons/hicolor" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(icons / relative, destination)
        launchers = bundle / "share/applications"
        launchers.mkdir(parents=True)
        desktop = launchers / f"{APP_ID}.desktop"
        shutil.copyfile(RESOURCES / "linux" / desktop.name, desktop)
        if shutil.which("desktop-file-validate"):
            subprocess.run(["desktop-file-validate", str(desktop)], check=True)
        notices = bundle / "share/doc/subroutine-lite"
        notices.mkdir(parents=True)
        write_licenses(notices, target, release)
        shutil.copyfile(RESOURCES / "linux/install", bundle / "install")
        (bundle / "install").chmod(0o755)
        temporary_archive = staging / archive.name
        with tarfile.open(temporary_archive, "w:gz", format=tarfile.PAX_FORMAT) as tar:
            tar.add(bundle, arcname="subroutine-lite", filter=public_metadata)
        shutil.copyfile(temporary_archive, archive)
    print(f"Built (not installed or launched): {archive}")
    print('Extract, then explicitly run ./subroutine-lite/install [--prefix /absolute/path].')
    print('Default install prefix: ~/.local. Native system libraries must be installed separately.')


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Stage a Linux bundle without user-specific paths or archive ownership."
    )
    parser.add_argument("build", type=Path)
    parser.add_argument("target", choices=("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"))
    parser.add_argument("--release", action="store_true")
    args = parser.parse_args()
    package(args.build, args.target, args.release)
