//! Building an actor export (`kairo.fed.actor-export.v1`,
//! `specs/DECISIONS.md` §26 sub-decision 7): an actor's genesis and
//! every key event, exactly as stored. Shared by `kairo actor export`
//! (client) and relayed inventories (server, §29 sub-decision 2).

use kairo_core::ActorId;
use kairo_federation_core::{ActorExport, BatchedStatement};
use kairo_store::{ActorStore, FilesystemStore, StoreError};

/// Why an export couldn't be built.
#[derive(Debug)]
pub(crate) enum ExportError {
    Store(StoreError),
    /// A stored statement isn't valid UTF-8 JSON.
    Statement {
        id: String,
        message: String,
    },
}

pub(crate) fn build_actor_export(
    store: &FilesystemStore,
    actor: &ActorId,
) -> Result<ActorExport, ExportError> {
    let genesis = store.get_actor(actor).map_err(ExportError::Store)?;
    let key_events = store
        .actor_key_event_ids(actor)
        .map_err(ExportError::Store)?
        .into_iter()
        .map(|id| {
            let bytes = store.get_statement_bytes(&id).map_err(ExportError::Store)?;
            let bad = |message: String| ExportError::Statement {
                id: id.to_string(),
                message,
            };
            let text = String::from_utf8(bytes).map_err(|error| bad(error.to_string()))?;
            let statement = serde_json::value::RawValue::from_string(text)
                .map_err(|error| bad(error.to_string()))?;
            Ok(BatchedStatement { id, statement })
        })
        .collect::<Result<Vec<_>, ExportError>>()?;
    Ok(ActorExport {
        schema: Default::default(),
        actor: actor.clone(),
        genesis: kairo_identity::json::ActorGenesisJson::from_body(&genesis),
        key_events,
    })
}
