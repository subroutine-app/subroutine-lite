use std::marker::PhantomData;

use anyhow::Context as _;
use sqlx::{
    Arguments as _, Encode, PgConnection, PgExecutor, Postgres, Row as _, Type,
    postgres::{PgArguments, PgRow},
};
use uuid::Uuid;

use super::TenantScope;

pub(crate) trait Record: Sized {
    const TABLE: &'static str;

    const COLUMNS: &'static [&'static str];

    const ORDER_BY: &'static str = "created_at";

    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error>;

    fn write<'a>(&'a self, columns: &mut Columns<'a>);
}

pub(crate) struct DeltaRows<T> {
    pub(crate) changed: Vec<T>,
    pub(crate) deleted: Vec<Uuid>,
}

pub(crate) struct Columns<'a> {
    names: Vec<&'static str>,
    values: PgArguments,
    error: Option<sqlx::error::BoxDynError>,
    _record: PhantomData<&'a ()>,
}

impl<'a> Columns<'a> {
    fn new(capacity: usize) -> Self {
        let mut values = PgArguments::default();
        values.reserve(capacity, 0);
        Self {
            names: Vec::with_capacity(capacity),
            values,
            error: None,
            _record: PhantomData,
        }
    }

    pub(crate) fn set<T>(&mut self, name: &'static str, value: T) -> &mut Self
    where
        T: 'a + Encode<'a, Postgres> + Type<Postgres>,
    {
        self.names.push(name);
        if let Err(e) = self.values.add(value) {
            self.error.get_or_insert(e);
        }
        self
    }
}

fn select_sql<R: Record>(filter: &str) -> String {
    format!(
        "SELECT {} FROM {} WHERE {filter}",
        R::COLUMNS.join(", "),
        R::TABLE
    )
}

fn fetch_all_sql<R: Record>() -> String {
    format!(
        "{} ORDER BY {}",
        select_sql::<R>("user_id = $1 AND deleted = FALSE"),
        R::ORDER_BY
    )
}

fn fetch_by_id_sql<R: Record>() -> String {
    select_sql::<R>("user_id = $1 AND id = $2 AND deleted = FALSE")
}

fn delta_sql<R: Record>() -> String {
    format!(
        "SELECT {}, deleted FROM {} \
         WHERE user_id = $1 AND change_seq > $2 AND change_seq <= $3 \
         ORDER BY change_seq, id",
        R::COLUMNS.join(", "),
        R::TABLE
    )
}

fn upsert_sql(table: &str, columns: &[&str]) -> String {
    let placeholders = (1..=columns.len() + 2)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");

    let assignments = columns
        .iter()
        .filter(|column| **column != "id")
        .map(|column| format!("{column} = EXCLUDED.{column}"))
        .chain([
            "deleted = FALSE".to_owned(),
            "change_seq = EXCLUDED.change_seq".to_owned(),
            "updated_at = NOW()".to_owned(),
        ])
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "INSERT INTO {table} ({}, user_id, change_seq, updated_at) \
         VALUES ({placeholders}, NOW()) \
         ON CONFLICT (id) DO UPDATE SET {assignments} \
         WHERE {table}.user_id = EXCLUDED.user_id",
        columns.join(", "),
    )
}

fn soft_delete_sql(table: &str) -> String {
    format!(
        "UPDATE {table} SET deleted = TRUE, change_seq = $3, updated_at = NOW() \
         WHERE user_id = $1 AND id = $2 AND deleted = FALSE"
    )
}

pub(crate) async fn upsert<R: Record>(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    change_seq: i64,
    record: &R,
) -> anyhow::Result<()> {
    let mut columns = Columns::new(R::COLUMNS.len());
    record.write(&mut columns);
    debug_assert_eq!(
        columns.names,
        R::COLUMNS,
        "{}: `write` and `COLUMNS` disagree",
        R::TABLE
    );

    if let Some(error) = columns.error {
        return Err(sqlx::Error::Encode(error))
            .with_context(|| format!("encode {} columns", R::TABLE));
    }

    columns
        .values
        .add(user_id)
        .map_err(sqlx::Error::Encode)
        .with_context(|| format!("encode {} user_id", R::TABLE))?;
    columns
        .values
        .add(change_seq)
        .map_err(sqlx::Error::Encode)
        .with_context(|| format!("encode {} change_seq", R::TABLE))?;

    let sql = upsert_sql(R::TABLE, &columns.names);
    let affected = sqlx::query_with(&sql, columns.values)
        .execute(db)
        .await
        .with_context(|| format!("upsert into {}", R::TABLE))?
        .rows_affected();
    anyhow::ensure!(affected > 0, "resource identity is unavailable");
    Ok(())
}

pub(crate) async fn fetch_all_in<R: Record>(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> anyhow::Result<Vec<R>> {
    let sql = fetch_all_sql::<R>();
    let rows = sqlx::query(&sql)
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await
        .with_context(|| format!("fetch all from {}", R::TABLE))?;

    rows.iter()
        .map(R::from_row)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("decode rows from {}", R::TABLE))
}

pub(crate) async fn fetch_all<R: Record>(scope: &TenantScope) -> anyhow::Result<Vec<R>> {
    let sql = fetch_all_sql::<R>();
    let rows = sqlx::query(&sql)
        .bind(scope.user_id)
        .fetch_all(&scope.pool)
        .await
        .with_context(|| format!("fetch all from {}", R::TABLE))?;

    rows.iter()
        .map(R::from_row)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("decode rows from {}", R::TABLE))
}

pub(crate) async fn fetch_delta_in<R: Record>(
    conn: &mut PgConnection,
    user_id: Uuid,
    since: i64,
    current_seq: i64,
) -> anyhow::Result<DeltaRows<R>> {
    let sql = delta_sql::<R>();
    let rows = sqlx::query(&sql)
        .bind(user_id)
        .bind(since)
        .bind(current_seq)
        .fetch_all(&mut *conn)
        .await
        .with_context(|| format!("fetch delta from {}", R::TABLE))?;

    let mut changed = Vec::with_capacity(rows.len());
    let mut deleted = Vec::new();
    for row in &rows {
        if row.try_get::<bool, _>("deleted")? {
            deleted.push(row.try_get("id")?);
        } else {
            changed.push(R::from_row(row)?);
        }
    }
    Ok(DeltaRows { changed, deleted })
}

pub(crate) async fn fetch_by_id_in<R: Record>(
    conn: &mut PgConnection,
    user_id: Uuid,
    id: Uuid,
) -> anyhow::Result<Option<R>> {
    let sql = fetch_by_id_sql::<R>();
    let row = sqlx::query(&sql)
        .bind(user_id)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .with_context(|| format!("fetch {} by id", R::TABLE))?;

    row.as_ref()
        .map(R::from_row)
        .transpose()
        .with_context(|| format!("decode row from {}", R::TABLE))
}

pub(crate) async fn identity_exists_in<R: Record>(
    conn: &mut PgConnection,
    user_id: Uuid,
    id: Uuid,
) -> anyhow::Result<bool> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {} WHERE user_id = $1 AND id = $2)",
        R::TABLE
    );
    sqlx::query_scalar(&sql)
        .bind(user_id)
        .bind(id)
        .fetch_one(conn)
        .await
        .with_context(|| format!("check {} identity", R::TABLE))
}

pub(crate) async fn fetch_by_id<R: Record>(
    scope: &TenantScope,
    id: Uuid,
) -> anyhow::Result<Option<R>> {
    let sql = fetch_by_id_sql::<R>();
    let row = sqlx::query(&sql)
        .bind(scope.user_id)
        .bind(id)
        .fetch_optional(&scope.pool)
        .await
        .with_context(|| format!("fetch {} by id", R::TABLE))?;

    row.as_ref()
        .map(R::from_row)
        .transpose()
        .with_context(|| format!("decode row from {}", R::TABLE))
}

pub(crate) async fn soft_delete<R: Record>(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    change_seq: i64,
    id: Uuid,
) -> anyhow::Result<bool> {
    let sql = soft_delete_sql(R::TABLE);
    let affected = sqlx::query(&sql)
        .bind(user_id)
        .bind(id)
        .bind(change_seq)
        .execute(db)
        .await
        .with_context(|| format!("soft delete from {}", R::TABLE))?
        .rows_affected();
    Ok(affected > 0)
}
