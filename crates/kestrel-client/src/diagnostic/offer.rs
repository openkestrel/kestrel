use std::io::{BufRead, Write};

use anyhow::{Context as _, Result};

use super::Invocation;
use super::step::{Step, Word};

/// Nothing runs unless a step is chosen, every missing input is typed, and a destructive step
/// gets an explicit yes. Answers the arguments to launch `kestrel` with.
pub fn chosen(
    steps: &[Step],
    invocation: &Invocation,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<Option<Vec<String>>> {
    let runnable: Vec<&Step> = steps.iter().filter(|step| step.command.is_some()).collect();
    if runnable.is_empty() {
        return Ok(None);
    }
    let step = loop {
        write!(output, "run a step? its number, or nothing for none: ")?;
        output.flush()?;
        let Some(answer) = answered(input, output)?.filter(|answer| !answer.is_empty()) else {
            return Ok(None);
        };
        if let Some(step) = answer
            .parse::<usize>()
            .ok()
            .and_then(|number| runnable.get(number.wrapping_sub(1)))
        {
            break *step;
        }
    };

    let mut argv = Vec::new();
    for word in step.command.iter().flatten() {
        match word {
            Word::Given(value) => argv.push(value.clone()),
            Word::Asked(asked) => {
                write!(output, "{}: ", asked.name)?;
                output.flush()?;
                let Some(value) = answered(input, output)?.filter(|value| !value.is_empty()) else {
                    return Ok(None);
                };
                argv.extend(asked.flag.clone());
                argv.push(value);
            }
        }
    }
    argv.extend(step.scope(invocation, true));
    if let Some(consequence) = &step.consequence {
        write!(output, "{consequence} {}? [y/N] ", step.says)?;
        output.flush()?;
        let yes = answered(input, output)?
            .is_some_and(|answer| matches!(answer.to_lowercase().as_str(), "y" | "yes"));
        if !yes {
            return Ok(None);
        }
    }
    Ok(Some(argv))
}

fn answered(input: &mut impl BufRead, output: &mut impl Write) -> Result<Option<String>> {
    let mut line = String::new();
    if input.read_line(&mut line).context("reading the answer")? == 0 {
        writeln!(output)?;
        return Ok(None);
    }
    Ok(Some(line.trim().to_owned()))
}

pub fn launched(argv: Vec<String>) -> Result<()> {
    std::process::Command::new(std::env::current_exe().context("finding this kestrel")?)
        .args(argv)
        .status()
        .context("running the chosen step")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::step::steps;
    use super::super::tests::{invocation, missing_project, stopping};
    use super::*;

    fn chose(
        diagnostic: &kestrel_operator_types::Diagnostic,
        typed: &str,
    ) -> (Option<Vec<String>>, String) {
        let invocation = invocation();
        let mut said = Vec::new();
        let argv = chosen(
            &steps(diagnostic, &invocation, false),
            &invocation,
            &mut typed.as_bytes(),
            &mut said,
        )
        .expect("a choice");
        (argv, String::from_utf8(said).expect("text"))
    }

    #[test]
    fn a_terminal_collects_what_is_missing() {
        let (argv, said) = chose(&missing_project(), "1\nhttps://example.com/repo\nmain\n");

        assert_eq!(
            argv.expect("the step should run"),
            [
                "project",
                "declare",
                "absent",
                "--repository",
                "https://example.com/repo",
                "--branch",
                "main",
                "--organization",
                "Acme East",
                "--control-plane",
                "http://127.0.0.1:7718",
            ]
        );
        assert!(said.contains("repository: "), "{said}");
    }

    #[test]
    fn nothing_runs_unless_a_step_is_chosen() {
        assert_eq!(chose(&missing_project(), "\n").0, None);
        assert_eq!(chose(&missing_project(), "").0, None);
        assert_eq!(chose(&missing_project(), "1\n\n").0, None);
    }

    #[test]
    fn a_destructive_step_needs_a_yes() {
        let (declined, asked) = chose(&stopping(), "2\n\n");
        assert_eq!(declined, None);
        assert!(asked.contains("[y/N]"), "{asked}");

        let (stopped, _) = chose(&stopping(), "2\nyes\n");
        assert_eq!(
            stopped.expect("the stop should run")[..3],
            ["session", "stop", "s"]
        );
    }
}
