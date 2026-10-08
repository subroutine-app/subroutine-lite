use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use super::parse_uuid;
use crate::{LocalStoreError, Result, WorkspaceIdentity, WorkspaceSpec, normalize_server_origin};

pub(super) fn initialize_identity(
    connection: &mut Connection,
    workspace: WorkspaceSpec,
) -> Result<WorkspaceIdentity> {
    let normalized = match workspace {
        WorkspaceSpec::Local => WorkspaceSpec::Local,
        WorkspaceSpec::Remote {
            server_origin,
            account_id,
        } => {
            if account_id.is_nil() {
                return Err(LocalStoreError::InvalidAccountIdentity);
            }
            WorkspaceSpec::Remote {
                server_origin: normalize_server_origin(&server_origin)?,
                account_id,
            }
        }
    };

    let existing = connection
        .query_row(
            "SELECT workspace_kind, workspace_id, client_id, server_origin, account_id
             FROM workspace WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?;

    if let Some((kind, workspace_id, client_id, origin, account_id)) = existing {
        let workspace_id = parse_uuid(&workspace_id, "workspace_id")?;
        let client_id = parse_uuid(&client_id, "client_id")?;
        let identity = match (kind.as_str(), origin, account_id) {
            ("local", None, None) => WorkspaceIdentity::Local {
                workspace_id,
                client_id,
            },
            ("remote", Some(server_origin), Some(account_id)) => WorkspaceIdentity::Remote {
                workspace_id,
                client_id,
                server_origin,
                account_id: parse_uuid(&account_id, "account_id")?,
            },
            _ => {
                return Err(LocalStoreError::WorkspaceMismatch(
                    "stored workspace metadata is inconsistent".into(),
                ));
            }
        };
        verify_workspace(&identity, &normalized)?;
        return Ok(identity);
    }

    let workspace_id = Uuid::new_v4();
    let client_id = Uuid::new_v4();
    let identity = match normalized {
        WorkspaceSpec::Local => {
            connection.execute(
                "INSERT INTO workspace (
                    singleton, workspace_kind, workspace_id, client_id
                 ) VALUES (1, 'local', ?1, ?2)",
                params![workspace_id.to_string(), client_id.to_string()],
            )?;
            WorkspaceIdentity::Local {
                workspace_id,
                client_id,
            }
        }
        WorkspaceSpec::Remote {
            server_origin,
            account_id,
        } => {
            connection.execute(
                "INSERT INTO workspace (
                    singleton, workspace_kind, workspace_id, client_id, server_origin, account_id
                 ) VALUES (1, 'remote', ?1, ?2, ?3, ?4)",
                params![
                    workspace_id.to_string(),
                    client_id.to_string(),
                    server_origin,
                    account_id.to_string()
                ],
            )?;
            WorkspaceIdentity::Remote {
                workspace_id,
                client_id,
                server_origin,
                account_id,
            }
        }
    };
    Ok(identity)
}

fn verify_workspace(identity: &WorkspaceIdentity, expected: &WorkspaceSpec) -> Result<()> {
    let matches = match (identity, expected) {
        (WorkspaceIdentity::Local { .. }, WorkspaceSpec::Local) => true,
        (
            WorkspaceIdentity::Remote {
                server_origin,
                account_id,
                ..
            },
            WorkspaceSpec::Remote {
                server_origin: expected_origin,
                account_id: expected_account,
            },
        ) => server_origin == expected_origin && account_id == expected_account,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(LocalStoreError::WorkspaceMismatch(format!(
            "stored {identity:?}, requested {expected:?}"
        )))
    }
}
