use sha2::{Digest, Sha256};
use std::path::PathBuf;
use url::Url;
use uuid::Uuid;

use crate::{LocalStoreError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceSpec {
    Local,
    Remote {
        server_origin: String,
        account_id: Uuid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceIdentity {
    Local {
        workspace_id: Uuid,
        client_id: Uuid,
    },
    Remote {
        workspace_id: Uuid,
        client_id: Uuid,
        server_origin: String,
        account_id: Uuid,
    },
}

impl WorkspaceIdentity {
    pub fn workspace_id(&self) -> Uuid {
        match self {
            Self::Local { workspace_id, .. } | Self::Remote { workspace_id, .. } => *workspace_id,
        }
    }

    pub fn client_id(&self) -> Uuid {
        match self {
            Self::Local { client_id, .. } | Self::Remote { client_id, .. } => *client_id,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceLocator {
    root: PathBuf,
}

impl WorkspaceLocator {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path_for(&self, workspace: &WorkspaceSpec) -> Result<PathBuf> {
        match workspace {
            WorkspaceSpec::Local => Ok(self.root.join("local.sqlite3")),
            WorkspaceSpec::Remote {
                server_origin,
                account_id,
            } => {
                if account_id.is_nil() {
                    return Err(LocalStoreError::InvalidAccountIdentity);
                }
                let origin = normalize_server_origin(server_origin)?;
                let mut hasher = Sha256::new();
                hasher.update(origin.as_bytes());
                hasher.update([0]);
                hasher.update(account_id.as_bytes());
                let digest = hasher.finalize();
                let hash = digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                Ok(self.root.join(format!("remote-{hash}.sqlite3")))
            }
        }
    }
}

pub fn normalize_server_origin(value: &str) -> Result<String> {
    let url = Url::parse(value.trim())
        .map_err(|error| LocalStoreError::InvalidServerOrigin(error.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(LocalStoreError::InvalidServerOrigin(
            "only absolute http and https origins are supported".into(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(LocalStoreError::InvalidServerOrigin(
            "origin must not contain credentials, a path, a query, or a fragment".into(),
        ));
    }
    Ok(url.origin().ascii_serialization())
}
