//! Generates the operator wire types from `openapi/operator/` into `OUT_DIR` (ADR-0059).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

use anyhow::{Context as _, Result, anyhow};
use openapi_to_rust::type_mapping::{DateStrategy, UuidStrategy};
use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
};
use serde_json::Value;

#[path = "src/document.rs"]
mod document;

fn main() -> Result<()> {
    let fragments = Path::new(&env::var("CARGO_MANIFEST_DIR")?).join("../../openapi/operator");
    println!("cargo::rerun-if-changed={}", fragments.display());
    println!("cargo::rerun-if-changed=src/document.rs");
    println!("cargo::rerun-if-changed=build.rs");

    let document = document::read(&fragments)
        .with_context(|| format!("reading the operator document in {}", fragments.display()))?;
    let types = TypeMappingConfig {
        date_time: DateStrategy::String,
        uuid: UuidStrategy::Uuid,
        ..Default::default()
    };
    let mut analysis =
        SchemaAnalyzer::with_type_mapper(document.clone(), TypeMapper::new(types.clone()))?
            .analyze()?;
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        types,
        ..Default::default()
    });
    let source = allow_omitted_nullable_fields(&generator.generate(&mut analysis)?)?;

    let out = env::var("OUT_DIR")?;
    let out = Path::new(&out);
    fs::write(out.join("generated.rs"), includable(&source))?;
    fs::write(out.join("sse.rs"), sse_mappings(&document)?)?;
    Ok(())
}

// `include!` puts the file inside a module, where the inner attributes and docs the generator
// opens with are not allowed; `lib.rs` carries the lints they allowed.
fn includable(source: &str) -> String {
    source
        .lines()
        .skip_while(|line| line.starts_with("//!") || line.starts_with("#![") || line.is_empty())
        .fold(String::new(), |mut kept, line| {
            kept.push_str(line);
            kept.push('\n');
            kept
        })
}

fn allow_omitted_nullable_fields(source: &str) -> Result<String> {
    let generated = "encoded.keys().all(|key| input.contains_key(key))";
    let compatible = "encoded.iter().all(|(key, value)| input.contains_key(key) || (nulls_may_be_absent && value.is_null()))";
    if !source.contains(generated) {
        return Err(anyhow!(
            "generated oneOf matcher changed; update nullable-field compatibility"
        ));
    }

    Ok(source.replace(generated, compatible))
}

fn sse_mappings(document: &Value) -> Result<String> {
    let mut source = String::from("use crate::*;\n");
    let schemas = document["components"]["schemas"]
        .as_object()
        .ok_or_else(|| anyhow!("missing schemas"))?;
    for (name, schema) in schemas {
        let Some(variants) = schema["oneOf"]
            .as_array()
            .or_else(|| schema["anyOf"].as_array())
        else {
            continue;
        };
        if !variants
            .iter()
            .any(|variant| variant.get("x-sse-event").is_some())
        {
            continue;
        }
        let mut names = BTreeSet::new();
        let mut mappings = Vec::new();
        for variant in variants {
            let event = variant["x-sse-event"]
                .as_str()
                .ok_or_else(|| anyhow!("SSE variant needs x-sse-event"))?;
            if !names.insert(event) {
                return Err(anyhow!("duplicate SSE event {event} in {name}"));
            }
            let target = variant["$ref"]
                .as_str()
                .and_then(|s| s.strip_prefix("#/components/schemas/"))
                .ok_or_else(|| anyhow!("SSE variant needs a named schema"))?;
            mappings.push((event, target));
        }
        writeln!(
            source,
            "impl {name} {{ pub fn event_name(&self) -> &'static str {{ match self {{"
        )?;
        for (event, target) in &mappings {
            writeln!(source, "Self::{target}(..) => {event:?},")?;
        }
        source.push_str("} }\n pub fn from_sse(event: &str, data: &str) -> Option<Result<Self, serde_json::Error>> { Some(match event {\n");
        for (event, target) in &mappings {
            writeln!(
                source,
                "{event:?} => serde_json::from_str(data).map(Self::{target}),"
            )?;
        }
        source.push_str("_ => return None,\n}) } }\n");
    }
    Ok(source)
}
