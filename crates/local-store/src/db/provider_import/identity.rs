use std::collections::HashMap;

use rusqlite::{Transaction, params};
use uuid::Uuid;

use super::resource::Kind;
use crate::{LocalStoreError, Result, db::parse_uuid};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Presence {
    Present,
    Excluded,
    Ignored,
}

pub(super) struct Entry {
    pub external_id: String,
    pub version: Option<String>,
    pub id: Uuid,
    pub kind: Kind,
    pub presence: Presence,
}

impl Entry {
    pub fn load(tx: &Transaction<'_>, provider: &str) -> Result<HashMap<String, Self>> {
        let mut statement = tx.prepare(
            "SELECT external_id, external_version, internal_uuid, item_type, ignored
             FROM integration_entries WHERE provider_id = ?1",
        )?;
        let rows = statement.query_map([provider], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (external_id, version, id, item_type, ignored) = row?;
            let (kind, presence) = match item_type.strip_prefix("excluded:") {
                Some(kind) => (kind, Presence::Excluded),
                None => (item_type.as_str(), Presence::Present),
            };
            let kind = match kind {
                "action" => Kind::Action,
                "event" => Kind::Event,
                "marker" => Kind::Marker,
                _ => {
                    return Err(LocalStoreError::InvalidMutation(format!(
                        "unknown provider item type {item_type}"
                    )));
                }
            };
            Ok((
                external_id.clone(),
                Self {
                    external_id,
                    version,
                    id: parse_uuid(&id, "provider item identity")?,
                    kind,
                    presence: if ignored { Presence::Ignored } else { presence },
                },
            ))
        })
        .collect()
    }

    pub fn save(&self, tx: &Transaction<'_>, provider: &str) -> Result<()> {
        // A missing present row is a tombstone; a policy exclusion may return.
        // Keep that distinction in item_type without changing the shared schema.
        let item_type = match self.presence {
            Presence::Excluded => format!("excluded:{}", self.kind.name()),
            Presence::Present | Presence::Ignored => self.kind.name().to_owned(),
        };
        tx.execute(
            "INSERT INTO integration_entries
             (provider_id, external_id, external_version, internal_uuid, item_type, ignored)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(provider_id, external_id) DO UPDATE SET
                 external_version = excluded.external_version,
                 internal_uuid = excluded.internal_uuid,
                 item_type = excluded.item_type,
                 ignored = excluded.ignored",
            params![
                provider,
                self.external_id,
                self.version,
                self.id.to_string(),
                item_type,
                self.presence == Presence::Ignored
            ],
        )?;
        Ok(())
    }
}
