//! Scalar functions exposed by the x12 worker, registered under `x12.main`.

mod delimiters;
mod transaction_type;

use vgi::Worker;

/// Register every scalar function on the worker.
pub fn register(worker: &mut Worker) {
    worker.register_scalar(delimiters::DelimitersFn);
    worker.register_scalar(transaction_type::TransactionTypeFn);
}
