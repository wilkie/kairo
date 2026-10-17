//! `kairo federation ...` runners: this node's federation publish
//! list (`specs/DECISIONS.md` §16 sub-decision 3,
//! `specs/PHASE_2_FEDERATION.md` slice 1).
//!
//! The list is local operator state in the store, so these commands
//! always run direct; there is no daemon route for them.

use kairo_core::{ObjectId, Timestamp};
use kairo_store::{PublishedObject, PublishedObjectStore};

use crate::cli::FederationCommand;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

/// Printed whenever an object stops being served, so nobody reads
/// `unpublish` as a recall.
const UNPUBLISH_NOTE: &str = "peers that already fetched this object keep their copy";

pub(crate) fn run_federation_command(
    command: FederationCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        FederationCommand::Publish { object } => run_publish(&parse_object(object)?, paths),
        FederationCommand::Unpublish { object } => run_unpublish(&parse_object(object)?, paths),
        FederationCommand::Published => run_published(paths),
    }
}

fn parse_object(object: String) -> Result<ObjectId, CliError> {
    ObjectId::new(object.clone()).map_err(|source| CliError::ParseObjectId { object, source })
}

fn run_publish(object: &ObjectId, paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let added = store
        .publish_object(object, Timestamp::now())
        .map_err(|source| CliError::PublishObject {
            object: object.clone(),
            source,
        })?;
    let status = if added {
        "published"
    } else {
        "already_published"
    };
    Ok(format!("object = {object}\nstatus = {status}\n"))
}

fn run_unpublish(object: &ObjectId, paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let removed = store
        .unpublish_object(object)
        .map_err(|source| CliError::UnpublishObject {
            object: object.clone(),
            source,
        })?;
    if removed {
        Ok(format!(
            "object = {object}\nstatus = unpublished\nnote = {UNPUBLISH_NOTE}\n"
        ))
    } else {
        Ok(format!("object = {object}\nstatus = not_published\n"))
    }
}

fn run_published(paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let objects = store
        .list_published_objects()
        .map_err(|source| CliError::ListPublishedObjects { source })?;
    Ok(format_published(&objects))
}

fn format_published(objects: &[PublishedObject]) -> String {
    let mut output = format!("published = {}\n", objects.len());
    for entry in objects {
        output.push_str(&format!(
            "  object={} published_at={}\n",
            entry.object_id, entry.published_at
        ));
    }
    output
}
