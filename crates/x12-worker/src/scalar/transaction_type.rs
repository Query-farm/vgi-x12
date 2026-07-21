//! `transaction_type(content) -> VARCHAR` — the first ST01 (X12) or UNH02
//! message type (EDIFACT) in inline content. NULL when none is found.

use std::sync::Arc;

use arrow_array::builder::StringBuilder;
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::DataType;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::content_bytes;

pub struct TransactionTypeFn;

impl ScalarFunction for TransactionTypeFn {
    fn name(&self) -> &str {
        "transaction_type"
    }

    fn metadata(&self) -> FunctionMetadata {
        let examples = vec![FunctionExample {
            sql: "SELECT x12.main.transaction_type('ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*1*0*P*:~GS*HP*S*R*20240101*1200*1*X*005010X221A1~ST*835*0001~SE*1*0001~GE*1*1~IEA*1*1~') AS tx_type;".into(),
            description: "Detect that an inline interchange carries an 835.".into(),
            expected_output: None,
        }];
        let mut tags = crate::meta::object_tags(
            "Detect EDI Transaction Type",
            "Detect the transaction set type of inline X12 or UN/EDIFACT content without \
             fully parsing the body: returns the first X12 ST01 transaction set identifier \
             ('835', '837', '270', '271', '850', '997', '999', …) or, for an EDIFACT \
             interchange, the UNH02 message type ('ORDERS', 'INVOIC', …). Useful for \
             routing / triage. Returns NULL when no transaction header is found or the \
             content is unrecognized (never an error). Pass the content from a column, e.g. \
             via read_text().",
            "Detect the EDI transaction type of inline content — first X12 ST01 or EDIFACT \
             UNH02 message type; NULL when none is found.",
            "transaction type, detect, route, triage, st01, unh, message type, x12, \
             edifact, 835, 837, 270, 271, 850",
            "Interchange sniffers",
        );
        tags.push((
            "vgi.example_queries".into(),
            crate::meta::example_queries_tag(&examples),
        ));
        FunctionMetadata {
            description: "Detect the transaction type (first ST01, or EDIFACT UNH02 message type) \
                          of inline content"
                .into(),
            examples,
            return_type: Some(DataType::Utf8),
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![ArgSpec::column(
            "content",
            0,
            "varchar",
            "Inline interchange content. Returns the first X12 ST01 transaction set identifier or \
             the EDIFACT UNH02 message type; NULL when none is found. Typically the content column \
             from read_text(...).",
        )]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Utf8))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut out = StringBuilder::new();
        for i in 0..rows {
            match content_bytes(col, i)? {
                Some(bytes) => {
                    let t = x12_core::envelope::first_transaction_type(&bytes);
                    if t.is_empty() {
                        out.append_null();
                    } else {
                        out.append_value(&t);
                    }
                }
                None => out.append_null(),
            }
        }
        let arr: ArrayRef = Arc::new(out.finish());
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::test_support::run_scalar_text;
    use arrow_array::cast::AsArray;
    use arrow_array::Array;

    #[test]
    fn detects_835_and_edifact() {
        let x12 = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*1*0*P*:~GS*HP*S*R*20240101*1200*1*X*005010X221A1~ST*835*0001~SE*1*0001~GE*1*1~IEA*1*1~";
        let edi = "UNB+UNOA:1+S+R+240101:1200+REF'UNH+M1+ORDERS:D:96A:UN'UNT+1+M1'UNZ+1+REF'";
        let out = run_scalar_text(
            &TransactionTypeFn,
            &[Some(x12), Some(edi), Some("nope"), None],
        )
        .unwrap();
        let s = out.as_string::<i32>();
        assert_eq!(s.value(0), "835");
        assert_eq!(s.value(1), "ORDERS");
        assert!(out.is_null(2));
        assert!(out.is_null(3));
    }
}
