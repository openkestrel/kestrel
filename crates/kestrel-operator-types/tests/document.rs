use std::{fs, path::PathBuf};

use kestrel_operator_types::document::bundle;
use serde_json::Value;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/document")
        .join(path)
}

#[test]
fn a_split_document_bundles_into_one_whose_refs_name_its_components() {
    let expected: Value =
        serde_json::from_str(&fs::read_to_string(fixture("bundled.json")).unwrap()).unwrap();

    assert_eq!(bundle(&fixture("split/operator.json")).unwrap(), expected);
}

#[test]
fn a_schema_name_two_files_define_is_refused() {
    let error = bundle(&fixture("collision/operator.json")).unwrap_err();

    assert!(error.to_string().contains("Thing"), "{error}");
}

#[test]
fn a_file_beside_the_root_that_no_reference_reaches_is_refused() {
    let error = bundle(&fixture("unreached/operator.json")).unwrap_err();

    assert!(error.to_string().contains("stray.json"), "{error}");
}
