use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fmt::Write as _,
    fs,
    path::PathBuf,
};

use chrono::{Datelike, NaiveDate};
use serde::Deserialize;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    start_year: i32,
    end_year: i32,
    holidays_version: String,
    regions: Vec<Region>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Region {
    id: String,
    country: String,
    subdivision: Option<String>,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    schema_version: u32,
    source: Source,
    manifest: Manifest,
    calendars: Vec<Calendar>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    package: String,
    version: String,
    language: String,
    categories: Vec<String>,
    observed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Calendar {
    id: String,
    supported_start_year: i32,
    supported_end_year: i32,
    years: BTreeMap<i32, Vec<Day>>,
}

#[derive(Deserialize)]
struct Day(String, Vec<String>);

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=data/manifest.json");
    println!("cargo:rerun-if-changed=data/holidays.json");
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string("data/manifest.json")?)?;
    let bundle: Bundle = serde_json::from_str(&fs::read_to_string("data/holidays.json")?)?;
    validate(&manifest, &bundle)?;
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    fs::write(output.join("holidays.rs"), generate(&bundle)?)?;
    Ok(())
}

fn validate(manifest: &Manifest, bundle: &Bundle) -> Result<()> {
    if &bundle.manifest != manifest {
        return Err(
            "Holiday manifest changed; run uv run --no-project script/generate-holidays.py".into(),
        );
    }
    if bundle.schema_version != 1
        || bundle.source.package != "holidays"
        || bundle.source.version != manifest.holidays_version
        || bundle.source.version.is_empty()
        || bundle.source.language != "en_US"
        || bundle.source.categories != ["public"]
        || !bundle.source.observed
    {
        return Err("Expected schema 1, pinned English PUBLIC holidays with observed dates".into());
    }
    if !(2..=9998).contains(&manifest.start_year)
        || !(manifest.start_year..=9998).contains(&manifest.end_year)
        || manifest.regions.is_empty()
    {
        return Err("Invalid holiday manifest years or empty region list".into());
    }

    let mut expected = BTreeSet::new();
    for region in &manifest.regions {
        let canonical = match &region.subdivision {
            Some(subdivision) if !subdivision.is_empty() => {
                format!("{}-{subdivision}", region.country)
            }
            Some(_) => return Err("Empty subdivision; use null for national only".into()),
            None => region.country.clone(),
        };
        if region.id != canonical
            || region.country.len() != 2
            || !region.country.bytes().all(|byte| byte.is_ascii_uppercase())
            || !region
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || region.name.trim().is_empty()
            || !expected.insert(region.id.as_str())
        {
            return Err(format!("Invalid or duplicate holiday region: {}", region.id).into());
        }
        if region.subdivision.is_none()
            && !region.name.contains("national only")
            && !region.name.contains("federal only")
        {
            return Err(format!(
                "Country-level region must say national/federal only: {}",
                region.id
            )
            .into());
        }
    }
    for calendar in &bundle.calendars {
        if !expected.remove(calendar.id.as_str()) {
            return Err(format!("Unlisted or duplicate holiday calendar: {}", calendar.id).into());
        }
        if calendar.supported_start_year > manifest.start_year - 1
            || calendar.supported_end_year < manifest.end_year + 1
            || !calendar
                .years
                .keys()
                .copied()
                .eq(manifest.start_year..=manifest.end_year)
        {
            return Err(
                format!("Incomplete holiday coverage or padding for {}", calendar.id).into(),
            );
        }
        for (year, days) in &calendar.years {
            if days.is_empty() || days.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
                return Err(format!(
                    "Empty, unsorted or duplicate holiday dates: {}/{year}",
                    calendar.id
                )
                .into());
            }
            for Day(date, names) in days {
                if parse_date(date)?.year() != *year
                    || names.is_empty()
                    || names.iter().any(|name| name.trim().is_empty())
                    || names.windows(2).any(|pair| pair[0] >= pair[1])
                {
                    return Err(
                        format!("Invalid holiday date/names: {}/{date}", calendar.id).into(),
                    );
                }
            }
        }
    }
    if !expected.is_empty() {
        return Err(format!("Missing holiday calendars: {expected:?}").into());
    }
    Ok(())
}

fn parse_date(date: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|error| format!("Invalid holiday date {date:?}: {error}"))?;
    if date.len() != 10 || parsed.year() <= 0 || parsed.format("%Y-%m-%d").to_string() != date {
        return Err(format!("Expected YYYY-MM-DD holiday date, got {date:?}").into());
    }
    Ok(parsed)
}

fn generate(bundle: &Bundle) -> Result<String> {
    let mut output = String::new();
    writeln!(
        output,
        "pub const START_YEAR: i32 = {};",
        bundle.manifest.start_year
    )?;
    writeln!(
        output,
        "pub const END_YEAR: i32 = {};",
        bundle.manifest.end_year
    )?;
    writeln!(output, "pub static REGIONS: &[Region] = &[")?;
    for region in &bundle.manifest.regions {
        writeln!(
            output,
            "Region {{ id: {:?}, country: {:?}, name: {:?} }},",
            region.id, region.country, region.name
        )?;
    }
    writeln!(output, "];\n")?;

    let mut map = phf_codegen::Map::new();
    for (index, calendar) in bundle.calendars.iter().enumerate() {
        writeln!(output, "static HOLIDAYS_{index}: &[Holiday] = &[")?;
        for Day(date, names) in calendar.years.values().flatten() {
            let date = parse_date(date)?;
            writeln!(
                output,
                "Holiday {{ date: NaiveDate::from_ymd_opt({}, {}, {}).expect(\"validated holiday date\"), title: {:?} }},",
                date.year(),
                date.month(),
                date.day(),
                names.join("; ")
            )?;
        }
        writeln!(output, "];\n")?;
        map.entry(calendar.id.as_str(), format!("HOLIDAYS_{index}"));
    }
    writeln!(
        output,
        "static HOLIDAYS: phf::Map<&'static str, &'static [Holiday]> = {};",
        map.build()
    )?;
    Ok(output)
}
