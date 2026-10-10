// Allow only the lint patterns emitted by the pinned generator.
#[allow(
    unused_variables,
    clippy::double_must_use,
    clippy::redundant_field_names,
    clippy::collapsible_if,
    clippy::nonminimal_bool
)]
mod generated;
mod sse;
pub use generated::*;

#[cfg(test)]
mod tests {
    use super::Diagnostic;

    #[test]
    fn diagnostic_accepts_a_new_nullable_context_field_omitted_by_an_older_client() {
        let input = serde_json::json!({
            "kind": "setup_gap",
            "message": "name the Operator",
            "field": null,
            "context": {
                "prerequisite": "operator",
                "resource": null,
                "reference": null,
                "organization": null,
                "harness": null,
                "method": null,
                "sign_in": null
            },
            "next_steps": []
        });

        assert!(matches!(
            serde_json::from_value::<Diagnostic>(input),
            Ok(Diagnostic::SetupGapDiagnostic(_))
        ));
    }
}
