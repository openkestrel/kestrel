use crate::support;

use std::fs;

#[test]
fn a_default_build_carries_line_tables_for_workspace_code_and_no_debug_info_for_dependencies() {
    let manifest = workspace_manifest();
    let dev = &manifest["profile"]["dev"];
    let dependencies = &dev["package"]["*"];

    assert_eq!(dev["debug"].as_str(), Some("line-tables-only"));
    assert_eq!(dependencies["debug"].as_bool(), Some(false));
    assert_eq!(
        dependencies["strip"].as_str(),
        Some("none"),
        "Cargo's implicit strip of debuginfo breaks proc-macro dylibs on macOS before rustc 1.98"
    );
}

#[test]
fn the_full_debug_override_restores_workspace_and_dependency_debug_info() {
    let manifest = workspace_manifest();
    let full_debug = &manifest["profile"]["full-debug"];

    assert_eq!(full_debug["inherits"].as_str(), Some("dev"));
    assert_eq!(full_debug["debug"].as_bool(), Some(true));
    assert_eq!(full_debug["package"]["*"]["debug"].as_bool(), Some(true));
}

fn workspace_manifest() -> toml::Value {
    let manifest = support::crate_root().join("../../Cargo.toml");
    toml::from_str(&fs::read_to_string(manifest).expect("a readable workspace manifest"))
        .expect("a valid workspace manifest")
}
