use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};

use subroutine_core::{Signal, SignalTemplate};

use super::convert::{recurrence_from_row, recurrence_to_sql};
use super::record::{Columns, Record};

impl Record for Signal {
    const TABLE: &'static str = "signals";
    const ORDER_BY: &'static str = "datetime";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "lineage_id",
        "template_id",
        "title",
        "content",
        "datetime",
        "recurrence",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Signal {
            id: row.try_get("id")?,
            lineage_id: row.try_get("lineage_id")?,
            template_id: row.try_get("template_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            datetime: row.try_get::<DateTime<Utc>, _>("datetime")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        columns
            .set("id", self.id)
            .set("lineage_id", self.lineage_id)
            .set("template_id", self.template_id)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("datetime", self.datetime)
            .set("recurrence", recurrence_to_sql(self.recurrence));
    }
}

impl Record for SignalTemplate {
    const TABLE: &'static str = "signal_templates";

    const COLUMNS: &'static [&'static str] =
        &["id", "lineage_id", "title", "content", "recurrence"];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(SignalTemplate {
            id: row.try_get("id")?,
            lineage_id: row.try_get("lineage_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        columns
            .set("id", self.id)
            .set("lineage_id", self.lineage_id)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("recurrence", recurrence_to_sql(self.recurrence));
    }
}
