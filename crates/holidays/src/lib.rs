use chrono::NaiveDate;
use subroutine_core::Marker;
use uuid::Uuid;

pub const UPSTREAM_NOTICE: &str = include_str!("../LICENSE.holidays");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    id: &'static str,
    country: &'static str,
    name: &'static str,
}

impl Region {
    pub const fn id(&self) -> &'static str {
        self.id
    }

    pub const fn country(&self) -> &'static str {
        self.country
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holiday {
    pub date: NaiveDate,
    pub title: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/holidays.rs"));

pub fn region(id: &str) -> Option<&'static Region> {
    REGIONS.iter().find(|region| region.id == id)
}

pub fn holidays(region: &Region) -> &'static [Holiday] {
    HOLIDAYS
        .get(region.id)
        .copied()
        .expect("generated holiday region has a calendar")
}

/// Creates the entire bundled range, without recurrence or an annual refill policy.
pub fn markers(region: &Region) -> Vec<Marker> {
    holidays(region)
        .iter()
        .map(|holiday| marker(region.id, holiday))
        .collect()
}

fn marker(region_id: &str, holiday: &Holiday) -> Marker {
    let source = format!("subroutine:holidays:{region_id}:{}", holiday.date);
    let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, source.as_bytes());
    Marker {
        id,
        lineage_id: id,
        template_id: None,
        title: holiday.title.to_owned(),
        content: None,
        date: holiday.date,
        end_date: None,
        recurrence: None,
        source_provider: Some("holidays".to_owned()),
        source_external_id: Some(source),
    }
}
