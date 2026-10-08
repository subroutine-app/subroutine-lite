#!/usr/bin/env python3
"""Build icons from the local SVG. Needs macOS, Python 3.9+, resvg 0.48.1 and iconutil."""

import argparse
import struct
import subprocess
import tempfile
from pathlib import Path

RESOURCES = Path(__file__).resolve().parents[1] / "crates/desktop/resources"
SOURCE = RESOURCES / "branding/subroutine_logo_cream_on_charcoal.svg"
APP_ID = "com.subroutine.SubroutineLite"
LINUX_SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)
WINDOWS_SIZES = (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)


def ico(images):
    offset = 6 + 16 * len(WINDOWS_SIZES)
    entries = []
    for size in WINDOWS_SIZES:
        data = images[size]
        entries.append(
            struct.pack(
                "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset
            )
        )
        offset += len(data)
    return (
        struct.pack("<HHH", 0, 1, len(entries))
        + b"".join(entries)
        + b"".join(images[size] for size in WINDOWS_SIZES)
    )


def icns(images, directory):
    iconset = directory / "Subroutine.iconset"
    iconset.mkdir()
    for size in (16, 32, 128, 256, 512):
        for scale, suffix in ((1, ""), (2, "@2x")):
            if size in (16, 32) and scale == 1:
                continue
            (iconset / f"icon_{size}x{size}{suffix}.png").write_bytes(
                images[size * scale]
            )
    output = directory / "Subroutine.icns"
    subprocess.run(
        ["iconutil", "-c", "icns", "-o", str(output), str(iconset)],
        check=True,
        timeout=30,
    )
    return output.read_bytes()


def generate():
    version = subprocess.check_output(
        ["resvg", "--version"], text=True, timeout=10
    ).strip()
    if version != "0.48.1":
        raise ValueError(f"Need resvg 0.48.1, got {version}")
    art = SOURCE.read_bytes().replace(b"\r\n", b"\n")
    images = {}
    with tempfile.TemporaryDirectory(prefix="subroutine-lite-icons-") as directory:
        source, output = Path(directory) / "source.svg", Path(directory) / "icon.png"
        source.write_bytes(art)
        for size in sorted(set(LINUX_SIZES + WINDOWS_SIZES)):
            subprocess.run(
                [
                    "resvg",
                    "--skip-system-fonts",
                    "--width",
                    str(size),
                    "--height",
                    str(size),
                    str(source),
                    str(output),
                ],
                check=True,
                timeout=30,
            )
            images[size] = output.read_bytes()
        mac_icon = icns(images, Path(directory))

    assets = {
        "branding/app-icon.png": images[512],
        "Subroutine.icns": mac_icon,
        "windows/Subroutine.ico": ico(images),
        f"linux/hicolor/scalable/apps/{APP_ID}.svg": art,
    }
    for size in LINUX_SIZES:
        assets[f"linux/hicolor/{size}x{size}/apps/{APP_ID}.png"] = images[size]
    return assets


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check without writing")
    args = parser.parse_args()
    assets = generate()
    stale = []
    for relative, data in assets.items():
        path = RESOURCES / relative
        if args.check:
            actual = path.read_bytes() if path.is_file() else None
            if actual is not None and path.suffix == ".svg":
                actual = actual.replace(b"\r\n", b"\n")
            if actual != data:
                stale.append(relative)
        else:
            path.write_bytes(data)
    if stale:
        raise SystemExit(
            "Stale icons; run python3 script/generate-icons.py:\n" + "\n".join(stale)
        )
    print(f"{'Checked' if args.check else 'Generated'} {len(assets)} icons")


if __name__ == "__main__":
    main()
