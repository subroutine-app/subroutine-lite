# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "holidays==0.106",
#   "python-dateutil==2.9.0.post0",
#   "six==1.17.0",
# ]
# ///

"""Refresh checked-in holiday data from the curated region/year manifest."""

import argparse
import json
import os
import re
import warnings
from datetime import date
from importlib.metadata import version
from pathlib import Path

import holidays

DEPENDENCIES = {
    "holidays": "0.106",
    "python-dateutil": "2.9.0.post0",
    "six": "1.17.0",
}
DATA = Path(__file__).resolve().parents[1] / "crates/holidays/data"
LANGUAGE = "en_US"


def load_manifest(path):
    manifest = json.loads(path.read_text(encoding="utf-8"))
    start, end = manifest["start_year"], manifest["end_year"]
    if type(start) is not int or type(end) is not int or not 2 <= start <= end <= 9998:
        raise ValueError(
            "Manifest requires 2 <= start_year <= end_year <= 9998 (inclusive)"
        )
    if manifest["holidays_version"] != DEPENDENCIES["holidays"]:
        raise ValueError(
            "Manifest holidays_version must match the pinned generator dependency"
        )
    supported = holidays.list_supported_countries(include_aliases=False)
    seen = set()
    if not manifest["regions"]:
        raise ValueError("Manifest must select at least one region")
    for region in manifest["regions"]:
        country, subdivision = region["country"], region["subdivision"]
        if country not in supported:
            raise ValueError(f"Unknown canonical country: {country}")
        if subdivision is not None and subdivision not in supported[country]:
            raise ValueError(f"Unknown canonical subdivision: {country}/{subdivision}")
        expected_id = country if subdivision is None else f"{country}-{subdivision}"
        if region["id"] != expected_id or not re.fullmatch(
            r"[A-Za-z0-9-]+", expected_id
        ):
            raise ValueError(
                f"Region ID must be the canonical ASCII country/subdivision: {expected_id}"
            )
        if expected_id in seen or not region["name"].strip():
            raise ValueError(f"Duplicate region or empty label: {expected_id}")
        if subdivision is None and not any(
            label in region["name"] for label in ("national only", "federal only")
        ):
            raise ValueError(
                f"Country-level label must explicitly say national/federal only: {country}"
            )
        seen.add(expected_id)
    return manifest


def calendar(region, start_year, end_year):
    options = {
        "subdiv": region["subdivision"],
        "categories": holidays.PUBLIC,
        "observed": True,
        "expand": False,
        "language": LANGUAGE,
    }
    metadata = holidays.country_holidays(region["country"], **options)
    first, last = start_year - 1, end_year + 1
    if first < metadata.start_year or last > metadata.end_year:
        raise ValueError(
            f"{region['id']} supports {metadata.start_year}–{metadata.end_year}, "
            f"not the requested {start_year}–{end_year} plus one-year boundary padding"
        )
    populated = holidays.country_holidays(
        region["country"], years=range(first, last + 1), **options
    )
    years = {str(year): [] for year in range(start_year, end_year + 1)}
    start, end = date(start_year, 1, 1), date(end_year + 1, 1, 1)
    for day in sorted(populated):
        if start <= day < end:
            names = sorted(set(populated.get_list(day)))
            if not names or any(not name.strip() for name in names):
                raise ValueError(f"Empty holiday name for {region['id']} on {day}")
            years[str(day.year)].append([day.isoformat(), names])
    for year, days in years.items():
        if not days:
            raise ValueError(
                f"No holidays for {region['id']}/{year}; refusing unclear coverage"
            )
    return {
        "id": region["id"],
        "supported_start_year": metadata.start_year,
        "supported_end_year": metadata.end_year,
        "years": years,
    }


def generate(manifest):
    return {
        "schema_version": 1,
        "source": {
            "package": "holidays",
            "version": DEPENDENCIES["holidays"],
            "language": LANGUAGE,
            "categories": [holidays.PUBLIC],
            "observed": True,
        },
        "manifest": manifest,
        "calendars": [
            calendar(region, manifest["start_year"], manifest["end_year"])
            for region in manifest["regions"]
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DATA / "manifest.json")
    parser.add_argument("--output", type=Path, default=DATA / "holidays.json")
    parser.add_argument("--check", action="store_true", help="compare without writing")
    args = parser.parse_args()
    try:
        for package, expected in DEPENDENCIES.items():
            if (actual := version(package)) != expected:
                raise ValueError(f"Need {package}=={expected}, found {actual}")
        # Unsupported locales otherwise consult the generator machine's environment.
        for key in ("LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"):
            os.environ[key] = LANGUAGE
        with warnings.catch_warnings():
            warnings.simplefilter("error")
            manifest = load_manifest(args.manifest)
            catalog = generate(manifest)
        encoded = (
            json.dumps(catalog, ensure_ascii=False, separators=(",", ":")) + "\n"
        ).encode("utf-8")
        if args.check:
            if not args.output.is_file() or args.output.read_bytes() != encoded:
                parser.exit(1, f"Catalog differs: regenerate {args.output}\n")
        else:
            args.output.write_bytes(encoded)
    except (
        KeyError,
        TypeError,
        ValueError,
        NotImplementedError,
        OSError,
        Warning,
    ) as error:
        parser.exit(1, f"Holiday generation failed: {error}\n")

    days = sum(
        len(days) for cal in catalog["calendars"] for days in cal["years"].values()
    )
    print(
        f"{len(catalog['calendars'])} regions, "
        f"{manifest['start_year']}–{manifest['end_year']}, {days} dates, "
        f"{len(encoded):,} bytes; {'verified' if args.check else 'wrote'} {args.output}"
    )


if __name__ == "__main__":
    main()
