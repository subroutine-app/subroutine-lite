
import argparse
import os
import plistlib
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from licenses import write_licenses

APP_ID = "com.subroutine.SubroutineLite"
APP_NAME = "Subroutine Lite.app"


def package(app, target, release, installer):
    try:
        metadata = plistlib.loads((app / "Contents/Info.plist").read_bytes())
        expected = {
            "CFBundleName": "Subroutine Lite",
            "CFBundleDisplayName": "Subroutine Lite",
            "CFBundleIdentifier": APP_ID,
            "CFBundleExecutable": "subroutine-lite",
            "CFBundlePackageType": "APPL",
        }
        if app.name != APP_NAME or any(metadata.get(k) != v for k, v in expected.items()):
            raise ValueError("bundle identity does not match Subroutine Lite")
        executable = app / "Contents/MacOS/subroutine-lite"
        if not executable.is_file() or not os.access(executable, os.X_OK):
            raise ValueError("missing executable subroutine-lite")
        icon_name = metadata["CFBundleIconFile"]
        if not icon_name.endswith(".icns"):
            icon_name += ".icns"
        icon = app / "Contents/Resources" / icon_name
        expected_icon = Path(__file__).resolve().parents[1] / "Subroutine.icns"
        if not icon.is_file() or icon.read_bytes() != expected_icon.read_bytes():
            raise ValueError("bundle icon does not match Subroutine.icns")
        version = metadata["CFBundleShortVersionString"]
        if not re.fullmatch(r"[0-9]+(?:\.[0-9]+){0,2}", version):
            raise ValueError("installer requires a numeric bundle version")
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"Expected bundle was not produced: {app}: {error}") from error

    resources = app / "Contents/Resources"
    resources.mkdir(exist_ok=True)
    write_licenses(resources, target, release or installer)

    # Seal the complete bundle, including licenses, before copying it into a pkg.
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)

    if installer:
        output = app.parent / f"Subroutine-Lite-{version}.pkg"
        with tempfile.TemporaryDirectory(prefix="subroutine-lite-pkg-") as directory:
            staging = Path(directory)
            payload = staging / "payload"
            destination = payload / "Applications" / APP_NAME
            destination.parent.mkdir(parents=True)
            shutil.copytree(app, destination, symlinks=True)
            components = staging / "components.plist"
            components.write_bytes(plistlib.dumps([{
                "RootRelativeBundlePath": f"Applications/{APP_NAME}",
                "BundleIsRelocatable": False,
                "BundleHasStrictIdentifier": True,
                "BundleIsVersionChecked": True,
                "BundleOverwriteAction": "upgrade",
            }]))
            package_file = staging / output.name
            subprocess.run([
                "pkgbuild", "--root", str(payload), "--component-plist", str(components),
                "--identifier", APP_ID, "--version", version,
                "--install-location", "/", "--ownership", "recommended", str(package_file),
            ], check=True)
            if not package_file.is_file() or not package_file.stat().st_size:
                raise SystemExit("pkgbuild did not produce an installer")
            shutil.copyfile(package_file, output)
        print(f"Built unsigned installer (not installed): {output}")
    print(f"Built (not installed or launched): {app}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Validate the Lite bundle and optionally build a non-relocatable installer."
    )
    parser.add_argument("app", type=Path)
    parser.add_argument("--target", required=True, choices=("aarch64-apple-darwin", "x86_64-apple-darwin"))
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--installer", action="store_true")
    args = parser.parse_args()
    package(args.app, args.target, args.release, args.installer)
