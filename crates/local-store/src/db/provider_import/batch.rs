use std::collections::{HashMap, HashSet, hash_map::Entry};

use subroutine_import::provider::{Imported, SyncItem};

use super::resource::Resource;
use crate::{LocalStoreError, Result};

pub(super) fn normalize(
    items: Vec<SyncItem>,
    excluded: &HashSet<String>,
    provider: &str,
) -> Result<Vec<Imported<Resource>>> {
    if excluded.iter().any(|id| id.trim().is_empty()) {
        return Err(LocalStoreError::InvalidMutation(format!(
            "provider {provider} returned a blank excluded identity; import left unchanged"
        )));
    }

    let mut positions = HashMap::new();
    let mut normalized: Vec<Imported<Resource>> = Vec::new();
    for (position, imported) in items.into_iter().map(Resource::imported).enumerate() {
        if imported.external_id.trim().is_empty() {
            return Err(LocalStoreError::InvalidMutation(format!(
                "provider {provider} returned a blank external identity at batch position {}; import left unchanged",
                position + 1
            )));
        }
        if excluded.contains(&imported.external_id) {
            return Err(LocalStoreError::InvalidMutation(format!(
                "provider {provider} both included and excluded identity {:?}; import left unchanged",
                imported.external_id
            )));
        }
        match positions.entry(imported.external_id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(normalized.len());
                normalized.push(imported);
            }
            Entry::Occupied(entry) => {
                let previous = &normalized[*entry.get()];
                let conflict = if previous.external_version != imported.external_version {
                    Some("opaque versions differ")
                } else if previous.item.kind() != imported.item.kind() {
                    Some("resource kinds differ")
                } else if !previous.item.same_payload(&imported.item) {
                    Some("payloads differ")
                } else {
                    None
                };
                if let Some(reason) = conflict {
                    return Err(LocalStoreError::InvalidMutation(format!(
                        "provider {provider} returned conflicting duplicates for identity {:?}: {reason}; import left unchanged",
                        imported.external_id
                    )));
                }
            }
        }
    }
    Ok(normalized)
}
