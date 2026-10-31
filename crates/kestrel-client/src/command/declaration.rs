use anyhow::{Context as _, Result};
use clap::Args;
use kestrel_operator_types as wire;
use serde_json::Value;

use super::Running;
use super::input::standard_input;
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::typed;

#[derive(Debug, Args)]
pub(crate) struct Apply {
    /// The declaration file, or `-` for standard input
    #[arg(short = 'f', long, value_name = "FILE")]
    file: String,
}

pub(crate) async fn run(running: Running<'_>, Apply { file }: Apply) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        invocation,
        ..
    } = running;
    let declaration = declaration(&file)?;
    let organization = scoping.resolve().await?.organization;
    if invocation.json {
        let applied: wire::AppliedDeclaration = api
            .post_as(
                &["organizations", &organization, "declaration"],
                &declaration,
            )
            .await?;
        show_applied_whole(presentation, &applied, &applied.admitting_outsiders)?;
    } else {
        let preview = api
            .post_as(
                &["organizations", &organization, "declaration", "preview"],
                &declaration,
            )
            .await?;
        show_declaration(preview);
        api.post_as::<wire::AppliedDeclaration>(
            &["organizations", &organization, "declaration"],
            &declaration,
        )
        .await?;
    }

    Ok(())
}

fn declaration(file: &str) -> Result<Value> {
    parsed(&read_file(file)?)
}

pub(super) fn parsed(text: &str) -> Result<Value> {
    yaml_serde::from_str(text).context(Failed::new(Exit::Usage, "reading the declaration file"))
}

pub(super) fn read_file(file: &str) -> Result<String> {
    if file == "-" {
        return standard_input();
    }

    std::fs::read_to_string(file)
        .with_context(|| Failed::new(Exit::Usage, format!("reading the declaration file {file}")))
}

pub(super) fn sign(action: &wire::DeclarationChangeAction) -> char {
    match action {
        wire::DeclarationChangeAction::Add => '+',
        wire::DeclarationChangeAction::Change => '~',
        wire::DeclarationChangeAction::Unchanged => '=',
        wire::DeclarationChangeAction::Remove => '-',
    }
}

pub(super) fn show_differences(differences: Vec<wire::DeclarationDifference>) {
    for difference in differences {
        println!("    {}", difference.field);
        for (sign, value) in [('-', difference.was), ('+', difference.becomes)] {
            for line in value.iter().flat_map(|value| value.lines()) {
                println!("      {sign} {line}");
            }
        }
    }
}

fn show_declaration(applied: wire::AppliedDeclaration) {
    for declared in applied.declarations {
        println!(
            "{} {} {}",
            sign(&declared.action),
            declared.kind,
            declared.name
        );
        show_differences(declared.differences);
    }
    for name in applied.admitting_outsiders {
        warn_of_outsiders(&name);
    }
}

pub(super) fn show_applied_whole(
    presentation: &Presentation,
    applied: &impl serde::Serialize,
    admitting_outsiders: &[String],
) -> Result<()> {
    for name in admitting_outsiders {
        warn_of_outsiders(name);
    }
    typed::whole(presentation, applied)
}

pub(super) fn warn_of_outsiders(name: &str) {
    eprintln!(
        "warning: the trigger {name} fires for events from people outside the organization. \
         Until 0.4, it is an unsupervised agent with your credentials on your repository, \
         briefed by whatever a stranger writes. Filter on the author_association GitHub \
         reports as OWNER, MEMBER or COLLABORATOR to decline strangers, or keep it as a \
         decision you made on purpose."
    );
}
