use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    io::Write as _,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Result, anyhow};
use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
    type_mapping::{DateStrategy, UuidStrategy},
};
use serde_json::Value;

fn main() -> Result<()> {
    let check = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err(anyhow!("usage: generate [--check]")),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let document: Value = serde_json::from_str(&fs::read_to_string(
        root.join("../../openapi/operator.json"),
    )?)?;
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
    let source = generator.generate(&mut analysis)?;
    let (sse, browser_sse) = sse_mappings(&document)?;
    write_output(
        &root.join("src/generated.rs"),
        &format_rust(&source)?,
        check,
    )?;
    write_output(&root.join("src/sse.rs"), &format_rust(&sse)?, check)?;
    write_output(
        &root.join("../../packages/client/src/operator/sse.gen.ts"),
        browser_sse.as_bytes(),
        check,
    )?;
    Ok(())
}

fn format_rust(source: &str) -> Result<Vec<u8>> {
    let mut formatter = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    formatter
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())?;
    let output = formatter.wait_with_output()?;
    if !output.status.success() {
        return Err(anyhow!("rustfmt failed"));
    }
    Ok(output.stdout)
}

fn write_output(path: &Path, content: &[u8], check: bool) -> Result<()> {
    if check {
        if fs::read(path).ok().as_deref() != Some(content) {
            return Err(anyhow!(
                "{} is stale; run cargo run -p kestrel-operator-types --bin generate",
                path.display()
            ));
        }
    } else {
        fs::write(path, content)?;
    }
    Ok(())
}

fn sse_mappings(document: &Value) -> Result<(String, String)> {
    let mut browser =
        String::from("// Generated from openapi/operator.json by the generate binary.\n");
    let mut source = String::from(
        "// Generated from openapi/operator.json by the generate binary.\nuse crate::*;\n",
    );
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
        writeln!(browser, "export const {name}Names = {{")?;
        for (event, target) in &mappings {
            writeln!(browser, "\t{target}: {event:?},")?;
        }
        browser.push_str("} as const;\n");
        writeln!(browser, "export type {name}Data = {{")?;
        for (event, target) in &mappings {
            writeln!(browser, "\t{event}: import(\"./generated\").{target};")?;
        }
        browser.push_str("};\n");
    }
    Ok((source, browser))
}
