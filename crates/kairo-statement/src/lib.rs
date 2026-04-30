//! Signed statement envelope primitives.

use kairo_core::{ActorId, KairoRef, StatementId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementEnvelope {
    id: StatementId,
    actor: ActorId,
    subject: KairoRef,
}

impl StatementEnvelope {
    pub fn new(id: StatementId, actor: ActorId, subject: KairoRef) -> Self {
        Self { id, actor, subject }
    }

    pub fn id(&self) -> &StatementId {
        &self.id
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn subject(&self) -> &KairoRef {
        &self.subject
    }
}
