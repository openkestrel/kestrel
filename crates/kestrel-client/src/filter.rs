//! The embedded jq (`--jq`), through jaq.
//!
//! A success answer reaches the filter through [`crate::output`], which is the one place
//! filtering happens, and the only thing that writes a response directly is a raw-content
//! exception named in the manual.

use anyhow::{Context as _, Result};
use jaq_core::data::JustLut;
use jaq_core::load::{Arena, Errors, File, Loader};
use jaq_core::{Compiler, Ctx, Vars};
use jaq_json::Val;
use serde::Deserialize as _;
use serde_json::Value;

use crate::exit::{Exit, Failed};

/// A compiled jq expression, ready to run over any number of answers.
pub struct Filter {
    program: jaq_core::Filter<JustLut<Val>>,
}

impl Filter {
    /// The pre-write hook: anything that can change the control plane validates its expression
    /// here first, so a malformed filter changes nothing, relays and interactive starts included.
    pub fn compile(expression: &str) -> Result<Self, String> {
        let defs = jaq_core::defs()
            .chain(jaq_std::defs())
            .chain(jaq_json::defs());
        let funs = jaq_core::funs::<JustLut<Val>>()
            .chain(jaq_std::funs::<JustLut<Val>>())
            .chain(jaq_json::funs::<JustLut<Val>>());
        let arena = Arena::default();
        let modules = Loader::new(defs)
            .load(
                &arena,
                File {
                    code: expression,
                    path: (),
                },
            )
            .map_err(load_failure)?;
        let program = Compiler::default()
            .with_funs(funs)
            .compile(modules)
            .map_err(compile_failure)?;

        Ok(Self { program })
    }

    /// Every value the expression yields for one answer is handed to `f`, in order.
    pub fn each(&self, answer: &Value, mut f: impl FnMut(Val) -> Result<()>) -> Result<()> {
        let input = Val::deserialize(answer)
            .context(Failed::new(Exit::Failure, "reading an answer to filter it"))?;
        let ctx = Ctx::<JustLut<Val>>::new(&self.program.lut, Vars::new([]));
        for yielded in self.program.id.run((ctx, input)) {
            let yielded = jaq_core::unwrap_valr(yielded).map_err(|error| {
                Failed::new(Exit::Failure, format!("the jq filter failed: {error}"))
            })?;
            f(yielded)?;
        }

        Ok(())
    }
}

fn load_failure(errors: Errors<&str, ()>) -> String {
    let mut said = Vec::new();
    for (_file, error) in errors {
        match error {
            jaq_core::load::Error::Io(raised) => said.extend(
                raised
                    .into_iter()
                    .map(|(path, reason)| format!("cannot load {path}: {reason}")),
            ),
            jaq_core::load::Error::Lex(raised) => {
                said.extend(raised.into_iter().map(|(expected, found)| {
                    format!("expected {}{}", expected.as_str(), near(found))
                }))
            }
            jaq_core::load::Error::Parse(raised) => {
                said.extend(raised.into_iter().map(|(expected, found)| {
                    format!("expected {}{}", expected.as_str(), near(found))
                }))
            }
        }
    }
    said.join("; ")
}

/// The token an expression stopped at, capped so a whole file cannot land in a diagnostic.
fn near(found: &str) -> String {
    let first = found.lines().next().unwrap_or_default();
    let capped: String = first.chars().take(40).collect();
    if capped.trim().is_empty() {
        String::new()
    } else {
        format!(" at `{capped}`")
    }
}

fn compile_failure(errors: jaq_core::compile::Errors<&str, ()>) -> String {
    let mut said = Vec::new();
    for (_file, raised) in errors {
        said.extend(raised.into_iter().map(|(found, undefined)| {
            format!("`{found}` names no filter ({})", undefined.as_str())
        }));
    }
    said.join("; ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// What the filter yields for one answer, rendered the way `--jq` prints it.
    fn run(expression: &str, answer: &Value) -> Vec<String> {
        let filter = Filter::compile(expression).expect("the filter should compile");
        let mut said = Vec::new();
        filter
            .each(answer, |value| {
                said.push(match value {
                    Val::BStr(bytes) | Val::TStr(bytes) => {
                        String::from_utf8_lossy(&bytes[..]).into_owned()
                    }
                    value => value.to_string(),
                });
                Ok(())
            })
            .expect("the filter should run");

        said
    }

    #[test]
    fn a_field_a_whole_collection_and_every_member_are_all_reachable() {
        let agents = json!([{ "name": "builder" }, { "name": "reviewer" }]);

        assert_eq!(run("length", &agents), ["2"]);
        assert_eq!(
            run("map(.name)", &agents),
            [r#"["builder","reviewer"]"#],
            "a collection is one array input"
        );
        assert_eq!(run(".[] | .name", &agents), ["builder", "reviewer"]);
        assert_eq!(run(".[0].name", &agents), ["builder"]);
    }

    #[test]
    fn a_filter_yielding_nothing_is_not_a_failure() {
        let agents = json!([{ "name": "builder" }]);

        assert!(run(r#".[] | select(.name == "nobody")"#, &agents).is_empty());
    }

    #[test]
    fn syntax_is_refused_when_the_filter_compiles() {
        assert!(Filter::compile(".[").is_err());
        assert!(Filter::compile("| nope").is_err());
        assert!(Filter::compile("map(").is_err());
    }

    #[test]
    fn a_filter_that_cannot_run_the_answer_is_an_error() {
        let filter = Filter::compile(r#""x" + 1"#).expect("a filter");
        assert!(
            filter
                .each(&json!({ "name": "globex" }), |_| Ok(()))
                .is_err(),
            "adding a number to a string did not fail"
        );
    }
}
