use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};

use subroutine_core::{Event, EventTemplate};

use super::convert::{duration_from_row, duration_to_sql, recurrence_from_row, recurrence_to_sql};
use super::record::{self, Columns, Record};
use super::{Sequenced, TenantScope};

impl Record for Event {
    const TABLE: &'static str = "events";
    const ORDER_BY: &'static str = "start_at";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "lineage_id",
        "template_id",
        "title",
        "content",
        "start_at",
        "duration",
        "recurrence",
        "source_provider",
        "source_external_id",
        "source_busy",
        "busy_override",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Event {
            id: row.try_get("id")?,
            lineage_id: row.try_get("lineage_id")?,
            template_id: row.try_get("template_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            start: row.try_get::<DateTime<Utc>, _>("start_at")?,
            duration: duration_from_row(row, "duration")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
            source_provider: row.try_get("source_provider")?,
            source_external_id: row.try_get("source_external_id")?,
            source_busy: row.try_get("source_busy")?,
            busy_override: row.try_get("busy_override")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        columns
            .set("id", self.id)
            .set("lineage_id", self.lineage_id)
            .set("template_id", self.template_id)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("start_at", self.start)
            .set("duration", duration_to_sql(self.duration))
            .set("recurrence", recurrence_to_sql(self.recurrence))
            .set("source_provider", &self.source_provider)
            .set("source_external_id", &self.source_external_id)
            .set("source_busy", self.source_busy)
            .set("busy_override", self.busy_override);
    }
}

impl Record for EventTemplate {
    const TABLE: &'static str = "event_templates";
    const ORDER_BY: &'static str = "sort_order, created_at";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "lineage_id",
        "sort_order",
        "title",
        "content",
        "duration",
        "recurrence",
        "source_provider",
        "source_external_id",
        "source_busy",
        "busy_override",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(EventTemplate {
            id: row.try_get("id")?,
            lineage_id: row.try_get("lineage_id")?,
            sort_order: row.try_get("sort_order")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            duration: duration_from_row(row, "duration")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
            source_provider: row.try_get("source_provider")?,
            source_external_id: row.try_get("source_external_id")?,
            source_busy: row.try_get("source_busy")?,
            busy_override: row.try_get("busy_override")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        columns
            .set("id", self.id)
            .set("lineage_id", self.lineage_id)
            .set("sort_order", self.sort_order)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("duration", duration_to_sql(self.duration))
            .set("recurrence", recurrence_to_sql(self.recurrence))
            .set("source_provider", &self.source_provider)
            .set("source_external_id", &self.source_external_id)
            .set("source_busy", self.source_busy)
            .set("busy_override", self.busy_override);
    }
}

pub(crate) async fn update_template_preserving_order(
    scope: &TenantScope,
    id: uuid::Uuid,
    mut template: EventTemplate,
) -> anyhow::Result<Option<Sequenced<EventTemplate>>> {
    let mut mutation = scope.begin_mutation().await?;
    let user_id = mutation.user_id();
    let previous =
        record::fetch_by_id_in::<EventTemplate>(mutation.connection(), user_id, id).await?;
    let Some(previous) = previous else {
        mutation.rollback().await?;
        return Ok(None);
    };
    template.id = id;
    template.sort_order = previous.sort_order;
    template.preserve_missing_availability(Some(&previous));
    let seq = mutation.next_change_seq().await?;
    record::upsert(mutation.connection(), user_id, seq, &template).await?;
    mutation.commit().await?;
    Ok(Some(Sequenced {
        value: template,
        seq,
    }))
}

pub(crate) async fn reorder_templates(
    scope: &TenantScope,
    ids: &[uuid::Uuid],
) -> anyhow::Result<Option<i64>> {
    let mut mutation = scope.begin_mutation().await?;
    let user_id = mutation.user_id();
    let active: Vec<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM event_templates
         WHERE user_id = $1 AND deleted = FALSE
         FOR UPDATE",
    )
    .bind(user_id)
    .fetch_all(mutation.connection())
    .await
    .context("lock event templates for reorder")?;
    if !is_exact_permutation(&active, ids) {
        mutation.rollback().await?;
        return Ok(None);
    }

    let seq = mutation.next_change_seq().await?;
    let affected = sqlx::query(
        "UPDATE event_templates AS template
         SET sort_order = ordered.position - 1,
             change_seq = $3,
             updated_at = NOW()
         FROM UNNEST($1::uuid[]) WITH ORDINALITY AS ordered(id, position)
         WHERE template.id = ordered.id
           AND template.user_id = $2
           AND template.deleted = FALSE",
    )
    .bind(ids)
    .bind(user_id)
    .bind(seq)
    .execute(mutation.connection())
    .await
    .context("reorder event templates")?
    .rows_affected();
    if affected != ids.len() as u64 {
        mutation.rollback().await?;
        return Ok(None);
    }

    mutation.commit().await?;
    Ok(Some(seq))
}

fn is_exact_permutation(active: &[uuid::Uuid], proposed: &[uuid::Uuid]) -> bool {
    active.len() == proposed.len() && active.iter().all(|id| proposed.contains(id))
}
