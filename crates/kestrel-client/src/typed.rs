//! Shows a decoded operator answer through columns that read its fields, beside `output`'s
//! dotted paths until every family is decoded.

use std::io::Write;

use anyhow::{Context as _, Result};
use serde::Serialize;

use crate::output::{self, Presentation, Shape};

pub struct Column<T>(pub &'static str, pub fn(&T) -> String);

pub enum Columns<T: 'static> {
    /// The one field a `$(…)` captures.
    Value(Column<T>),
    Rows(&'static [Column<T>]),
    Detail(&'static [Column<T>]),
}

impl<T> Columns<T> {
    fn shape(&self) -> Shape {
        match self {
            Columns::Value(_) => Shape::Value,
            Columns::Rows(_) => Shape::Rows,
            Columns::Detail(_) => Shape::Detail,
        }
    }

    fn all(&self) -> &[Column<T>] {
        match self {
            Columns::Value(column) => std::slice::from_ref(column),
            Columns::Rows(columns) | Columns::Detail(columns) => columns,
        }
    }

    fn cells(&self, record: &T) -> Vec<String> {
        self.all().iter().map(|column| (column.1)(record)).collect()
    }
}

pub fn text(value: impl ToString) -> String {
    value.to_string()
}

pub fn or_absent<V: ToString>(value: Option<&V>) -> String {
    value.map_or_else(|| output::ABSENT.to_owned(), V::to_string)
}

pub fn joined<V: ToString>(values: &[V]) -> String {
    if values.is_empty() {
        return output::ABSENT.to_owned();
    }
    values
        .iter()
        .map(V::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// A record inside a record, written as the boundary wrote it; nothing reads as absent.
pub fn object<T: Serialize>(value: Option<&T>) -> String {
    value.map_or_else(|| output::ABSENT.to_owned(), serialized)
}

/// Records inside a record, each as the boundary wrote it; nothing reads as absent.
pub fn objects<T: Serialize>(values: &[T]) -> String {
    if values.is_empty() {
        return output::ABSENT.to_owned();
    }
    values.iter().map(serialized).collect::<Vec<_>>().join(",")
}

fn serialized<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("a nested record serializes")
}

pub fn collection<T: Serialize>(
    presentation: &Presentation,
    columns: &Columns<T>,
    records: &[T],
    empty: &str,
) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if records.is_empty() && matches!(presentation, Presentation::Human(_)) {
        writeln!(out, "{empty}")?;
        return Ok(());
    }
    written(&mut out, presentation, columns, &records, records)?;
    out.flush().context("writing to standard output")
}

pub fn show<T: Serialize>(
    presentation: &Presentation,
    columns: &Columns<T>,
    record: &T,
) -> Result<()> {
    let mut out = std::io::stdout().lock();
    written(
        &mut out,
        presentation,
        columns,
        record,
        std::slice::from_ref(record),
    )?;
    out.flush().context("writing to standard output")
}

/// Shows one record through columns that read it, while JSON answers what the boundary sent: a
/// record whose columns resolve something the record itself does not hold is shown through those.
pub fn show_as<T>(
    presentation: &Presentation,
    columns: &Columns<T>,
    whole: &impl Serialize,
    record: &T,
) -> Result<()> {
    let mut out = std::io::stdout().lock();
    written(
        &mut out,
        presentation,
        columns,
        whole,
        std::slice::from_ref(record),
    )?;
    out.flush().context("writing to standard output")
}

/// `whole` is the answer as the control plane gave it, which for one record is not an array.
fn written<T>(
    out: &mut impl Write,
    presentation: &Presentation,
    columns: &Columns<T>,
    whole: &impl Serialize,
    records: &[T],
) -> Result<()> {
    match presentation {
        Presentation::Json(_) => {
            output::document(out, presentation, &serde_json::to_value(whole)?)?
        }
        Presentation::Delimited => {
            for record in records {
                writeln!(out, "{}", output::delimited(&columns.cells(record)))?;
            }
        }
        Presentation::Human(width) => {
            let labels: Vec<String> = columns
                .all()
                .iter()
                .map(|column| column.0.to_owned())
                .collect();
            let rows: Vec<Vec<String>> =
                records.iter().map(|record| columns.cells(record)).collect();
            output::table(out, &columns.shape(), &labels, &rows, *width)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[derive(Serialize)]
    struct Agent {
        id: u8,
        name: &'static str,
        model: Option<&'static str>,
        tags: Vec<&'static str>,
    }

    const AGENTS: Columns<Agent> = Columns::Rows(&[
        Column("name", |agent| text(agent.name)),
        Column("model", |agent| or_absent(agent.model.as_ref())),
        Column("tags", |agent| joined(&agent.tags)),
    ]);
    const MODEL: Columns<Agent> =
        Columns::Value(Column("model", |agent| or_absent(agent.model.as_ref())));
    const AGENT: Columns<Agent> = Columns::Detail(&[
        Column("name", |agent| text(agent.name)),
        Column("model", |agent| or_absent(agent.model.as_ref())),
    ]);

    fn agents() -> Vec<Agent> {
        vec![
            Agent {
                id: 1,
                name: "builder",
                model: Some("claude-opus-5"),
                tags: vec!["a", "b"],
            },
            Agent {
                id: 2,
                name: "re\tviewer",
                model: None,
                tags: Vec::new(),
            },
        ]
    }

    fn said<T>(
        presentation: &Presentation,
        columns: &Columns<T>,
        whole: &impl Serialize,
        records: &[T],
    ) -> String {
        let mut out = Vec::new();
        written(&mut out, presentation, columns, whole, records).expect("written");
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn a_pipe_gets_each_records_columns_joined_by_tabs_with_a_dash_for_nothing() {
        let agents = agents();

        assert_eq!(
            said(&Presentation::Delimited, &AGENTS, &agents, &agents),
            "builder\tclaude-opus-5\ta,b\nre\\tviewer\t-\t-\n"
        );
        assert_eq!(
            said(&Presentation::Delimited, &MODEL, &agents[1], &agents[1..]),
            "-\n"
        );
    }

    #[test]
    fn a_terminal_gets_the_columns_under_their_labels() {
        let agents = agents();

        assert_eq!(
            said(&Presentation::Human(usize::MAX), &AGENTS, &agents, &agents),
            "name        model          tags\n\
             builder     claude-opus-5  a,b\n\
             re\\tviewer  -              -\n"
        );
        assert_eq!(
            said(
                &Presentation::Human(usize::MAX),
                &AGENT,
                &agents[0],
                &agents[..1]
            ),
            "name   builder\nmodel  claude-opus-5\n"
        );
    }

    #[test]
    fn json_is_the_whole_answer_and_not_the_columns() {
        let agents = agents();
        let none: [Agent; 0] = [];

        assert_eq!(
            said(&Presentation::Json(None), &MODEL, &agents[1], &agents[1..]),
            "{\"id\":2,\"name\":\"re\\tviewer\",\"model\":null,\"tags\":[]}\n"
        );
        assert_eq!(
            said(&Presentation::Json(None), &AGENTS, &agents, &agents)
                .lines()
                .count(),
            1
        );
        assert_eq!(
            said(&Presentation::Json(None), &AGENTS, &none, &none),
            "[]\n"
        );
    }
}
