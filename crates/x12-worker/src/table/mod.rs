//! Table functions exposed by the x12 worker, registered under `x12.main`.
//!
//! Every function shares the **path | text | bytes** input modes
//! ([`crate::source`]), the envelope-key carry-down, and one generic
//! row-to-batch producer ([`RowsProducer`]). A malformed interchange NEVER
//! aborts the query: parsing is total (panic-free) and degrades to fewer rows /
//! NULL validity flags rather than erroring (per-row error capture).

pub mod edifact;
pub mod envelope;
pub mod segments;
pub mod segments_elements;
pub mod shaped;

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use vgi::table_function::TableProducer;
use vgi::Worker;
use vgi_rpc::{OutputCollector, Result};

use crate::arrow_io::{build_batch, Cell};

/// Output rows emitted per `next_batch`.
const BATCH_ROWS: usize = 2048;

/// Register every table function on the worker.
pub fn register(worker: &mut Worker) {
    worker.register_table(segments::Segments);
    worker.register_table(segments_elements::SegmentsElements);
    worker.register_table(envelope::Envelope);
    worker.register_table(edifact::EdifactSegments);
    worker.register_table(edifact::EdifactEnvelope);
    for def in x12_core::shaped::REGISTRY {
        worker.register_table(shaped::Shaped::new(def));
    }
}

/// A generic [`TableProducer`] over a fully-materialized row set, paginating
/// `BATCH_ROWS` rows per `next_batch`. All correctness lives in the per-function
/// row builders; this just slices and builds Arrow.
pub struct RowsProducer {
    schema: SchemaRef,
    rows: Vec<Vec<Cell>>,
    cursor: usize,
}

impl RowsProducer {
    pub fn new(schema: SchemaRef, rows: Vec<Vec<Cell>>) -> Self {
        RowsProducer {
            schema,
            rows,
            cursor: 0,
        }
    }

    /// Slice the next `BATCH_ROWS` rows and build their batch, advancing the
    /// cursor; `None` when exhausted. The pagination core, shared by
    /// [`TableProducer::next_batch`] and unit-testable without the RPC plumbing.
    fn take_batch(&mut self) -> Result<Option<RecordBatch>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let end = (self.cursor + BATCH_ROWS).min(self.rows.len());
        let slice = &self.rows[self.cursor..end];
        self.cursor = end;
        Ok(Some(build_batch(&self.schema, slice)?))
    }
}

impl TableProducer for RowsProducer {
    fn next_batch(&mut self, _out: &mut OutputCollector) -> Result<Option<RecordBatch>> {
        self.take_batch()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_schema::{DataType, Field, Schema};
    use std::sync::Arc;

    /// Drain a producer over more rows than fit in one batch and assert the
    /// pagination yields every row exactly once across the batch boundaries
    /// (the resume invariant for a held-state producer).
    #[test]
    fn paginates_across_batch_boundaries() {
        let n = BATCH_ROWS * 2 + 7;
        let schema = Arc::new(Schema::new(vec![Field::new("v", DataType::Int64, true)]));
        let rows: Vec<Vec<Cell>> = (0..n as i64).map(|i| vec![Cell::I64(Some(i))]).collect();
        let mut p = RowsProducer::new(schema, rows);
        let mut total = 0usize;
        let mut batches = 0usize;
        while let Some(b) = p.take_batch().unwrap() {
            total += b.num_rows();
            batches += 1;
        }
        assert_eq!(total, n, "every row emitted exactly once");
        assert_eq!(batches, 3, "two full batches + one remainder");
        assert!(p.take_batch().unwrap().is_none());
    }
}
