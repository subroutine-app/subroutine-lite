use anyhow::{Context as _, bail};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DatasetRotation {
    pub(crate) previous_dataset_id: Uuid,
    pub(crate) dataset_id: Uuid,
    pub(crate) change_seq: i64,
    pub(crate) deleted_receipts: u64,
}

pub(crate) async fn rotate_dataset(
    pool: &PgPool,
    user_id: Uuid,
    actor: &str,
    reason: &str,
) -> anyhow::Result<DatasetRotation> {
    let actor = actor.trim();
    let reason = reason.trim();
    if user_id.is_nil() {
        bail!("user ID must not be nil");
    }
    if actor.is_empty() {
        bail!("actor must not be blank");
    }
    if reason.is_empty() {
        bail!("reason must not be blank");
    }

    let mut tx = pool.begin().await.context("begin dataset rotation")?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('subroutine:user:' || $1::uuid::text, 0))",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("acquire tenant lock for dataset rotation")?;
    let previous_dataset_id: Uuid =
        sqlx::query_scalar("SELECT dataset_id FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .context("lock dataset owner")?
            .with_context(|| format!("user {user_id} does not exist"))?;
    let deleted_receipts = sqlx::query("DELETE FROM mutation_receipts WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("delete receipts from previous dataset epoch")?
        .rows_affected();
    let dataset_id = Uuid::now_v7();
    let change_seq: i64 = sqlx::query_scalar(
        "UPDATE users \
         SET dataset_id = $2, change_seq = change_seq + 1, updated_at = NOW() \
         WHERE id = $1 \
         RETURNING change_seq",
    )
    .bind(user_id)
    .bind(dataset_id)
    .fetch_one(&mut *tx)
    .await
    .context("rotate tenant dataset identity")?;
    sqlx::query(
        "INSERT INTO administrative_audit_events \
         (id, event_type, actor, reason, target_user_id, details) \
         VALUES ($1, 'dataset_rotated', $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(actor)
    .bind(reason)
    .bind(user_id)
    .bind(serde_json::json!({
        "previous_dataset_id": previous_dataset_id,
        "dataset_id": dataset_id,
        "deleted_receipts": deleted_receipts,
    }))
    .execute(&mut *tx)
    .await
    .context("audit dataset rotation")?;
    tx.commit().await.context("commit dataset rotation")?;

    Ok(DatasetRotation {
        previous_dataset_id,
        dataset_id,
        change_seq,
        deleted_receipts,
    })
}

#[derive(Clone)]
pub(crate) struct TenantScope {
    pub(crate) pool: PgPool,
    pub(crate) user_id: Uuid,
}

#[derive(Debug)]
pub(crate) struct Sequenced<T> {
    pub(crate) value: T,
    pub(crate) seq: i64,
}

pub(crate) struct TenantMutation {
    tx: Transaction<'static, Postgres>,
    user_id: Uuid,
}

impl TenantScope {
    pub(crate) fn new(pool: PgPool, user_id: Uuid) -> Self {
        Self { pool, user_id }
    }

    pub(crate) async fn all_users(pool: &PgPool) -> anyhow::Result<Vec<Self>> {
        let user_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users ORDER BY id")
            .fetch_all(pool)
            .await
            .context("list tenants")?;
        Ok(user_ids
            .into_iter()
            .map(|user_id| Self::new(pool.clone(), user_id))
            .collect())
    }

    pub(crate) async fn begin_mutation(&self) -> anyhow::Result<TenantMutation> {
        let mut tx = self.pool.begin().await.context("begin tenant mutation")?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('subroutine:user:' || $1::uuid::text, 0))",
        )
        .bind(self.user_id)
        .execute(&mut *tx)
        .await
        .context("acquire tenant mutation lock")?;
        Ok(TenantMutation {
            tx,
            user_id: self.user_id,
        })
    }

    pub(crate) async fn current_change_seq(&self) -> anyhow::Result<i64> {
        sqlx::query_scalar("SELECT change_seq FROM users WHERE id = $1")
            .bind(self.user_id)
            .fetch_one(&self.pool)
            .await
            .context("read tenant change sequence")
    }

    pub(crate) async fn dataset_id(&self) -> anyhow::Result<Uuid> {
        sqlx::query_scalar("SELECT dataset_id FROM users WHERE id = $1")
            .bind(self.user_id)
            .fetch_one(&self.pool)
            .await
            .context("read tenant dataset identity")
    }

    pub(crate) async fn for_identity(
        pool: PgPool,
        issuer: &str,
        subject: &str,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !issuer.is_empty() && !subject.is_empty(),
            "external identity must be nonempty"
        );
        if let Some(user_id) =
            sqlx::query_scalar("SELECT id FROM users WHERE auth_issuer = $1 AND auth_subject = $2")
                .bind(issuer)
                .bind(subject)
                .fetch_optional(&pool)
                .await
                .context("resolve authenticated user")?
        {
            return Ok(Self::new(pool, user_id));
        }

        let mut tx = pool
            .begin()
            .await
            .context("begin authenticated user provisioning")?;
        let user_id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO users (id, auth_issuer, auth_subject) VALUES ($1, $2, $3) \
             ON CONFLICT (auth_issuer, auth_subject) DO NOTHING \
             RETURNING id",
        )
        .bind(Uuid::now_v7())
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&mut *tx)
        .await
        .context("provision authenticated user")?;
        let user_id = match user_id {
            Some(user_id) => {
                sqlx::query("INSERT INTO account_profiles (user_id) VALUES ($1)")
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await
                    .context("provision authenticated user profile")?;
                user_id
            }
            None => {
                sqlx::query_scalar(
                    "SELECT id FROM users WHERE auth_issuer = $1 AND auth_subject = $2",
                )
                .bind(issuer)
                .bind(subject)
                .fetch_one(&mut *tx)
                .await
                .context("resolve concurrently provisioned user")?
            }
        };
        tx.commit()
            .await
            .context("commit authenticated user provisioning")?;

        Ok(Self::new(pool, user_id))
    }
}

impl TenantMutation {
    pub(crate) fn connection(&mut self) -> &mut PgConnection {
        &mut self.tx
    }

    pub(crate) fn user_id(&self) -> Uuid {
        self.user_id
    }

    pub(crate) async fn identity(&mut self) -> anyhow::Result<(Uuid, i64)> {
        sqlx::query_as("SELECT dataset_id, change_seq FROM users WHERE id = $1")
            .bind(self.user_id)
            .fetch_one(&mut *self.tx)
            .await
            .context("read tenant dataset identity and change sequence in mutation")
    }

    pub(crate) async fn next_change_seq(&mut self) -> anyhow::Result<i64> {
        sqlx::query_scalar(
            "UPDATE users SET change_seq = change_seq + 1 WHERE id = $1 RETURNING change_seq",
        )
        .bind(self.user_id)
        .fetch_one(&mut *self.tx)
        .await
        .context("allocate tenant change sequence")
    }

    pub(crate) async fn commit(self) -> anyhow::Result<()> {
        self.tx.commit().await.context("commit tenant mutation")
    }

    pub(crate) async fn rollback(self) -> anyhow::Result<()> {
        self.tx
            .rollback()
            .await
            .context("roll back tenant mutation")
    }
}
