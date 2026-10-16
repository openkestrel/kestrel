use serde_json::Value;

/// The operator document, merged from its fragments as every consumer reads it.
pub fn operator() -> Value {
    kestrel_operator_types::document::read(&super::crate_root().join("../../openapi/operator"))
        .expect("a readable operator document")
}
