//! Object manifest and metadata types.

use kairo_core::{ObjectId, SnapshotId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectDependency {
    object: ObjectId,
    selector: ObjectDependencySelector,
}

impl ObjectDependency {
    pub fn new(object: ObjectId, selector: ObjectDependencySelector) -> Self {
        Self { object, selector }
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn selector(&self) -> &ObjectDependencySelector {
        &self.selector
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectDependencySelector {
    Version(String),
    Snapshot(SnapshotId),
}
