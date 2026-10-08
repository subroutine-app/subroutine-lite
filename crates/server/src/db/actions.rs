use anyhow::Context as _;
use chrono::NaiveTime;
use sqlx::{Row, postgres::PgRow};

use subroutine_core::{Action, ActionTemplate};

use super::convert::{
    opt_duration_from_row, opt_duration_to_sql, recurrence_from_row, recurrence_to_sql,
    schedule_point_from_row, schedule_point_to_sql,
};
use super::record::{self, Columns, Record};
use super::{Sequenced, TenantScope};

impl Record for Action {
    const TABLE: &'static str = "actions";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "recurrence_id",
        "routine_id",
        "template_id",
        "title",
        "content",
        "queued",
        "pinned",
        "start_at",
        "start_date",
        "duration",
        "completion",
        "recurrence",
        "source_provider",
        "source_external_id",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Action {
            id: row.try_get("id")?,
            recurrence_id: row.try_get("recurrence_id")?,
            routine_id: row.try_get("routine_id")?,
            template_id: row.try_get("template_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            queued: row.try_get("queued")?,
            pinned: row.try_get("pinned")?,
            start: schedule_point_from_row(row, "start_at", "start_date")?,
            duration: opt_duration_from_row(row, "duration")?,
            completion: row.try_get("completion")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
            source_provider: row.try_get("source_provider")?,
            source_external_id: row.try_get("source_external_id")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        let (start_at, start_date) = schedule_point_to_sql(self.start);
        columns
            .set("id", self.id)
            .set("recurrence_id", self.recurrence_id)
            .set("routine_id", self.routine_id)
            .set("template_id", self.template_id)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("queued", self.queued)
            .set("pinned", self.pinned)
            .set("start_at", start_at)
            .set("start_date", start_date)
            .set("duration", opt_duration_to_sql(self.duration))
            .set("completion", self.completion)
            .set("recurrence", recurrence_to_sql(self.recurrence))
            .set("source_provider", &self.source_provider)
            .set("source_external_id", &self.source_external_id);
    }
}

impl Record for ActionTemplate {
    const TABLE: &'static str = "action_templates";
    const ORDER_BY: &'static str = "sort_order, created_at";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "sort_order",
        "title",
        "content",
        "naive_time",
        "duration",
        "recurrence",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ActionTemplate {
            id: row.try_get("id")?,
            sort_order: row.try_get("sort_order")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            naive_time: row.try_get::<Option<NaiveTime>, _>("naive_time")?,
            duration: opt_duration_from_row(row, "duration")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        columns
            .set("id", self.id)
            .set("sort_order", self.sort_order)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("naive_time", self.naive_time)
            .set("duration", opt_duration_to_sql(self.duration))
            .set("recurrence", recurrence_to_sql(self.recurrence));
    }
}

pub(crate) async fn update_template_preserving_order(
    scope: &TenantScope,
    id: uuid::Uuid,
    mut template: ActionTemplate,
) -> anyhow::Result<Option<Sequenced<ActionTemplate>>> {
    let mut mutation = scope.begin_mutation().await?;
    let user_id = mutation.user_id();
    let sort_order: Option<i64> = sqlx::query_scalar(
        "SELECT sort_order FROM action_templates
         WHERE id = $1 AND user_id = $2 AND deleted = FALSE
         FOR UPDATE",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(mutation.connection())
    .await
    .context("lock action template for update")?;
    let Some(sort_order) = sort_order else {
        mutation.rollback().await?;
        return Ok(None);
    };
    template.id = id;
    template.sort_order = sort_order;
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
        "SELECT id FROM action_templates
         WHERE user_id = $1 AND deleted = FALSE
         FOR UPDATE",
    )
    .bind(user_id)
    .fetch_all(mutation.connection())
    .await
    .context("lock action templates for reorder")?;
    if !is_exact_permutation(&active, ids) {
        mutation.rollback().await?;
        return Ok(None);
    }

    let seq = mutation.next_change_seq().await?;
    let affected = sqlx::query(
        "UPDATE action_templates AS template
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
    .context("reorder action templates")?
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
