//! `x12_version()` — return the worker's version string.

use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_schema::DataType;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

pub struct X12Version;

impl ScalarFunction for X12Version {
    fn name(&self) -> &str {
        "x12_version"
    }

    fn metadata(&self) -> FunctionMetadata {
        FunctionMetadata {
            description: "Returns the x12 worker version string".into(),
            return_type: Some(DataType::Utf8),
            examples: vec![FunctionExample {
                sql: "SELECT x12.main.x12_version();".into(),
                description: "Return the x12 worker version string.".into(),
                expected_output: None,
            }],
            tags: {
                let mut tags = crate::meta::object_tags(
                    "X12 Worker Version",
                    "Return the version string of the running x12 worker binary (the worker's own \
                     build version, the crate's Cargo version — not the SDK/protocol version). The \
                     string is semver MAJOR.MINOR.PATCH. Argument-free and deterministic; always \
                     returns the same single VARCHAR (never NULL) for a given build. Useful for \
                     diagnostics and confirming which build is attached.",
                    "Return the x12 worker version string, e.g. `x12_version()` → '0.1.0'. \
                     Argument-free and deterministic.",
                    "version, build version, x12_version, diagnostics, worker version, semver",
                );
                tags.push((
                    "vgi.executable_examples".into(),
                    r#"[
  {
    "description": "Return the worker version string.",
    "sql": "SELECT x12.main.x12_version() AS version"
  }
]"#
                    .into(),
                ));
                tags
            },
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        Vec::new()
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Utf8))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let rows = batch.num_rows();
        let out: ArrayRef = Arc::new(StringArray::from(vec![x12_core::version(); rows]));
        RecordBatch::try_new(params.output_schema.clone(), vec![out])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
