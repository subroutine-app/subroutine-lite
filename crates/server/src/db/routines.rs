use anyhow::Context as _;
use sqlx::{PgConnection, PgExecutor, Row, postgres::PgRow};
use uuid::Uuid;

use subroutine_core::{CoreItem as _, Recurrence, Routine, RoutineStep, SchedulePoint};

use super::TenantScope;
use super::convert::{
    opt_duration_from_row, opt_duration_to_sql, recurrence_from_row, recurrence_to_sql,
    schedule_point_from_row, schedule_point_to_sql,
};
use super::record::{self, Columns, DeltaRows, Record};

struct RoutineRow {
    id: Uuid,
    recurrence_id: Uuid,
    title: String,
    content: Option<String>,
    target: Option<SchedulePoint>,
    recurrence: Option<Recurrence>,
}

impl RoutineRow {
    fn of(routine: &Routine) -> Self {
        Self {
            id: routine.id,
            recurrence_id: routine.lineage_id(),
            title: routine.title.clone(),
            content: routine.content.clone(),
            target: routine.target,
            recurrence: routine.recurrence,
        }
    }

    fn into_routine(self, steps: Vec<RoutineStep>) -> Routine {
        Routine {
            id: self.id,
            recurrence_id: self.recurrence_id,
            title: self.title,
            content: self.content,
            target: self.target,
            steps,
            recurrence: self.recurrence,
        }
    }
}

impl Record for RoutineRow {
    const TABLE: &'static str = "routines";
    const ORDER_BY: &'static str = "position NULLS LAST, created_at, id";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "recurrence_id",
        "title",
        "content",
        "target_at",
        "target_date",
        "recurrence",
    ];

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(RoutineRow {
            id: row.try_get("id")?,
            recurrence_id: row.try_get("recurrence_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            target: schedule_point_from_row(row, "target_at", "target_date")?,
            recurrence: recurrence_from_row(row, "recurrence")?,
        })
    }

    fn write<'a>(&'a self, columns: &mut Columns<'a>) {
        let (target_at, target_date) = schedule_point_to_sql(self.target);
        columns
            .set("id", self.id)
            .set("recurrence_id", self.recurrence_id)
            .set("title", &self.title)
            .set("content", &self.content)
            .set("target_at", target_at)
            .set("target_date", target_date)
            .set("recurrence", recurrence_to_sql(self.recurrence));
    }
}

const FETCH_STEPS_SQL: &str = "SELECT title, duration FROM routine_steps WHERE user_id = $1 AND routine_id = $2 ORDER BY position";
const DELETE_STEPS_SQL: &str = "DELETE FROM routine_steps WHERE user_id = $1 AND routine_id = $2";
const INSERT_STEP_SQL: &str =
    "INSERT INTO routine_steps (id, user_id, routine_id, title, duration, position)
     VALUES ($1, $2, $3, $4, $5, $6)";

const LOCK_ACTIVE_ROUTINES_SQL: &str =
    "SELECT id FROM routines WHERE user_id = $1 AND deleted = FALSE ORDER BY id FOR UPDATE";
const BUMP_ROUTINE_SQL: &str = "UPDATE routines SET change_seq = $1, updated_at = NOW() \
     WHERE user_id = $2 AND id = $3 AND deleted = FALSE";
const POSITION_ROUTINE_SQL: &str = "UPDATE routines SET position = $1, change_seq = $2, updated_at = NOW() \
     WHERE user_id = $3 AND id = $4 AND deleted = FALSE";

async fn fetch_steps_in(
    conn: &mut PgConnection,
    user_id: Uuid,
    routine_id: Uuid,
) -> anyhow::Result<Vec<RoutineStep>> {
    let rows = sqlx::query(FETCH_STEPS_SQL)
        .bind(user_id)
        .bind(routine_id)
        .fetch_all(&mut *conn)
        .await
        .context("fetch routine steps")?;

    rows.iter()
        .map(|row| {
            Ok(RoutineStep {
                title: row.try_get("title")?,
                duration: opt_duration_from_row(row, "duration")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .context("decode routine steps")
}

async fn fetch_steps(scope: &TenantScope, routine_id: Uuid) -> anyhow::Result<Vec<RoutineStep>> {
    let rows = sqlx::query(FETCH_STEPS_SQL)
        .bind(scope.user_id)
        .bind(routine_id)
        .fetch_all(&scope.pool)
        .await
        .context("fetch routine steps")?;

    rows.iter()
        .map(|row| {
            Ok(RoutineStep {
                title: row.try_get("title")?,
                duration: opt_duration_from_row(row, "duration")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .context("decode routine steps")
}

async fn replace_steps_in(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    change_seq: i64,
    routine_id: Uuid,
    steps: &[RoutineStep],
) -> anyhow::Result<()> {
    let bumped = sqlx::query(BUMP_ROUTINE_SQL)
        .bind(change_seq)
        .bind(user_id)
        .bind(routine_id)
        .execute(&mut *conn)
        .await
        .context("stamp routine step replacement")?
        .rows_affected();
    anyhow::ensure!(
        bumped == 1,
        "routine {routine_id} not found while replacing steps"
    );

    sqlx::query(DELETE_STEPS_SQL)
        .bind(user_id)
        .bind(routine_id)
        .execute(&mut *conn)
        .await
        .context("delete old routine steps")?;

    for (position, step) in steps.iter().enumerate() {
        sqlx::query(INSERT_STEP_SQL)
            .bind(Uuid::now_v7())
            .bind(user_id)
            .bind(routine_id)
            .bind(&step.title)
            .bind(opt_duration_to_sql(step.duration))
            .bind(position as i32)
            .execute(&mut *conn)
            .await
            .context("insert routine step")?;
    }
    Ok(())
}

pub(crate) async fn upsert_in(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    change_seq: i64,
    routine: &Routine,
) -> anyhow::Result<()> {
    record::upsert(&mut *conn, user_id, change_seq, &RoutineRow::of(routine)).await?;
    replace_steps_in(conn, user_id, change_seq, routine.id, &routine.steps).await
}

pub(crate) async fn fetch_all_in(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> anyhow::Result<Vec<Routine>> {
    let rows = record::fetch_all_in::<RoutineRow>(conn, user_id).await?;

    let mut routines = Vec::with_capacity(rows.len());
    for row in rows {
        let steps = fetch_steps_in(conn, user_id, row.id).await?;
        routines.push(row.into_routine(steps));
    }
    Ok(routines)
}

pub(crate) async fn fetch_all(scope: &TenantScope) -> anyhow::Result<Vec<Routine>> {
    let rows = record::fetch_all::<RoutineRow>(scope).await?;

    let mut routines = Vec::with_capacity(rows.len());
    for row in rows {
        let steps = fetch_steps(scope, row.id).await?;
        routines.push(row.into_routine(steps));
    }
    Ok(routines)
}

pub(crate) async fn fetch_order_in(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> anyhow::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM routines WHERE user_id = $1 AND deleted = FALSE \
         ORDER BY position NULLS LAST, created_at, id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await
    .context("fetch routine order")
}

pub(crate) async fn fetch_delta_in(
    conn: &mut PgConnection,
    user_id: Uuid,
    since: i64,
    current_seq: i64,
) -> anyhow::Result<DeltaRows<Routine>> {
    let rows = record::fetch_delta_in::<RoutineRow>(conn, user_id, since, current_seq).await?;
    let mut changed = Vec::with_capacity(rows.changed.len());
    for row in rows.changed {
        let steps = fetch_steps_in(conn, user_id, row.id).await?;
        changed.push(row.into_routine(steps));
    }
    Ok(DeltaRows {
        changed,
        deleted: rows.deleted,
    })
}

pub(crate) async fn reorder_in(
    conn: &mut PgConnection,
    user_id: Uuid,
    change_seq: i64,
    ids: &[Uuid],
) -> anyhow::Result<bool> {
    use std::collections::HashSet;

    if ids.is_empty()
        || ids.iter().any(Uuid::is_nil)
        || ids.iter().copied().collect::<HashSet<_>>().len() != ids.len()
    {
        return Ok(false);
    }

    let active_ids: Vec<Uuid> = sqlx::query_scalar(LOCK_ACTIVE_ROUTINES_SQL)
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await
        .context("lock active routines")?;
    let active: HashSet<_> = active_ids.into_iter().collect();
    let requested: HashSet<_> = ids.iter().copied().collect();
    if active != requested {
        return Ok(false);
    }

    for (position, id) in ids.iter().enumerate() {
        let result = sqlx::query(POSITION_ROUTINE_SQL)
            .bind(position as i64)
            .bind(change_seq)
            .bind(user_id)
            .bind(id)
            .execute(&mut *conn)
            .await
            .context("position routine")?;
        if result.rows_affected() != 1 {
            return Ok(false);
        }
    }

    Ok(true)
}

pub(crate) async fn reorder(scope: &TenantScope, ids: &[Uuid]) -> anyhow::Result<Option<i64>> {
    let mut mutation = scope.begin_mutation().await?;
    let user_id = mutation.user_id();
    let seq = mutation.next_change_seq().await?;
    if !reorder_in(mutation.connection(), user_id, seq, ids).await? {
        mutation.rollback().await?;
        return Ok(None);
    }

    mutation.commit().await?;
    Ok(Some(seq))
}

pub(crate) async fn fetch_by_id(scope: &TenantScope, id: Uuid) -> anyhow::Result<Option<Routine>> {
    let Some(row) = record::fetch_by_id::<RoutineRow>(scope, id).await? else {
        return Ok(None);
    };
    let steps = fetch_steps(scope, row.id).await?;
    Ok(Some(row.into_routine(steps)))
}

pub(crate) async fn soft_delete(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    change_seq: i64,
    id: Uuid,
) -> anyhow::Result<bool> {
    record::soft_delete::<RoutineRow>(db, user_id, change_seq, id).await
}
