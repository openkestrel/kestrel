use std::sync::LazyLock;

use kestrel_operator_types::HarnessCatalogueEntry;

static HARNESSES: LazyLock<Vec<HarnessCatalogueEntry>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("catalogue/harnesses.json"))
        .expect("the product harness catalogue conforms to the operator schema")
});

pub fn harnesses() -> &'static [HarnessCatalogueEntry] {
    &HARNESSES
}

pub fn sign_in_method(
    harness: &str,
    method: &str,
) -> anyhow::Result<&'static kestrel_operator_types::SignInMethod> {
    use crate::declined::{Constraint, Reason};

    let row = harnesses()
        .iter()
        .find(|row| row.name == harness)
        .ok_or_else(|| Reason::InvalidField {
            field: "harness",
            operation: "show_sign_in_method",
            constraint: Constraint::Offered,
            allowed: Some(harnesses().iter().map(|row| row.name.clone()).collect()),
            message: format!("the harness {harness} has no guided Sign-in Methods"),
        })?;
    Ok(row
        .sign_in_methods
        .iter()
        .find(|offered| offered.id == method)
        .ok_or_else(|| Reason::InvalidField {
            field: "method",
            operation: "show_sign_in_method",
            constraint: Constraint::Offered,
            allowed: Some(
                row.sign_in_methods
                    .iter()
                    .map(|offered| offered.id.clone())
                    .collect(),
            ),
            message: format!("the harness {harness} offers no Sign-in Method {method}"),
        })?)
}
