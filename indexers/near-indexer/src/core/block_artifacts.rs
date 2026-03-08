use anyhow::Result;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::ConnectionTrait;
use std::sync::Mutex;

use crate::core::block_facts::BlockFacts;
use crate::core::raw_projector::{RawBlockProjection, RawProjector};
use crate::core::receipt_graph_resolver::{ReceiptGraphResolver, ResolvedReceiptGraph};

pub struct BlockArtifacts {
    facts: BlockFacts,
    receipt_graph: Mutex<Option<ResolvedReceiptGraph>>,
    raw_projection: Mutex<Option<RawBlockProjection>>,
}

impl BlockArtifacts {
    pub fn new(facts: BlockFacts) -> Self {
        Self {
            facts,
            receipt_graph: Mutex::new(None),
            raw_projection: Mutex::new(None),
        }
    }

    pub fn facts(&self) -> &BlockFacts {
        &self.facts
    }

    pub async fn receipt_graph(
        &self,
        resolver: &ReceiptGraphResolver,
        msg: &StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<ResolvedReceiptGraph> {
        if let Some(cached) = self.receipt_graph.lock().unwrap().clone() {
            return Ok(cached);
        }

        let resolved = resolver.resolve_for_block(msg, &self.facts, db).await?;
        *self.receipt_graph.lock().unwrap() = Some(resolved.clone());
        Ok(resolved)
    }

    pub async fn raw_projection(
        &self,
        resolver: &ReceiptGraphResolver,
        msg: &StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<RawBlockProjection> {
        if let Some(cached) = self.raw_projection.lock().unwrap().clone() {
            return Ok(cached);
        }

        let resolved = self.receipt_graph(resolver, msg, db).await?;
        let projection = RawProjector::project(msg, &self.facts, &resolved, db).await?;
        *self.raw_projection.lock().unwrap() = Some(projection.clone());
        Ok(projection)
    }
}
