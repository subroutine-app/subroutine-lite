use chrono::NaiveDate;
use sqlx::{Row, postgres::PgRow};

use subroutine_core::{Marker, MarkerTemplate};

use super::convert::{recurrence_from_row, recurrence_to_sql};
use super::record::{Columns, Record};

impl Record for Marker {
    const TABLE: &'static str = "markers";
    const ORDER_BY: &'static str = "date";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "lineage_id",
        "template_id",
        "title",
        "content",
        "date",
        "end_date",
        "recurrence",
        "source_provider",
        "source_external_id",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Marker {
            id: row.try_get("id")?,
            lineage_id: row.try_get("lineage_id")?,
            template_id: row.try_get("template_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            date: row.try_get::<NaiveDate, _>("date")?,
            end_date: row.try_get::<Option<NaiveDate>, _>("end_date")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
            source_provider: row.try_get("source_provider")?,
            source_external_id: row.try_get("source_external_id")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        columns
            .set("id", self.id)
            .set("lineage_id", self.lineage_id)
            .set("template_id", self.template_id)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("date", self.date)
            .set("end_date", self.end_date)
            .set("recurrence", recurrence_to_sql(self.recurrence))
            .set("source_provider", &self.source_provider)
            .set("source_external_id", &self.source_external_id);
    }
}

impl Record for MarkerTemplate {
    const TABLE: &'static str = "marker_templates";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "lineage_id",
        "title",
        "content",
        "span_days",
        "recurrence",
        "source_provider",
        "source_external_id",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(MarkerTemplate {
            id: row.try_get("id")?,
            lineage_id: row.try_get("lineage_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            span_days: row.try_get::<i32, _>("span_days")? as u32,
            recurrence: recurrence_from_row(row, "recurrence")?,
            source_provider: row.try_get("source_provider")?,
            source_external_id: row.try_get("source_external_id")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        columns
            .set("id", self.id)
            .set("lineage_id", self.lineage_id)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("span_days", self.span_days.max(1) as i32)
            .set("recurrence", recurrence_to_sql(self.recurrence))
            .set("source_provider", &self.source_provider)
            .set("source_external_id", &self.source_external_id);
    }
}
