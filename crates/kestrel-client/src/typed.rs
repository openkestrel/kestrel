//! Shows a decoded operator answer through columns that read its fields.

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

/// A field that is itself a JSON value, shown as the value it is.
pub fn compound(value: &impl Serialize) -> String {
    output::rendered(serde_json::to_value(value).ok().as_ref())
}

/// The complete answer of a command with no columns to show it in.
pub fn whole(presentation: &Presentation, answer: &impl Serialize) -> Result<()> {
    let mut out = std::io::stdout().lock();
    output::document(&mut out, presentation, &serde_json::to_value(answer)?)?;
    out.flush().context("writing to standard output")
}

/// One record of a stream as it arrives, unclipped because a streamed entry is prose; a filter
/// runs per record, and a record it yields nothing for writes nothing at all.
pub fn record<T: Serialize>(
    out: &mut impl Write,
    presentation: &Presentation,
    columns: &Columns<T>,
    record: &T,
) -> Result<()> {
    match presentation {
        Presentation::Json(_) => {
            output::document(out, presentation, &serde_json::to_value(record)?)?
        }
        Presentation::Delimited => writeln!(out, "{}", output::delimited(&columns.cells(record)))?,
        Presentation::Human(_) => writeln!(out, "{}", columns.cells(record).join(output::GAP))?,
    }

    Ok(())
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

pub fn show_with<T: Serialize>(
    presentation: &Presentation,
    columns: &Columns<T>,
    record: &T,
    cell: impl Fn(&str, String) -> String,
) -> Result<()> {
    if presentation.json() {
        return show(presentation, columns, record);
    }
    let labels: Vec<String> = columns
        .all()
        .iter()
        .map(|column| column.0.to_owned())
        .collect();
    let cells: Vec<String> = columns
        .all()
        .iter()
        .map(|column| cell(column.0, (column.1)(record)))
        .collect();
    let mut out = std::io::stdout().lock();
    match presentation {
        Presentation::Human(width) => {
            output::table(&mut out, &columns.shape(), &labels, &[cells], *width)?
        }
        Presentation::Delimited => writeln!(out, "{}", output::delimited(&cells))?,
        Presentation::Json(_) => unreachable!(),
    }
    out.flush().context("writing to standard output")
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

    fn session(patch: serde_json::Value) -> kestrel_operator_types::Session {
        let mut record = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001", "workspace": "00000000-0000-0000-0000-000000000002",
            "name": "red-blue-abcdefgh", "agent": "builder", "harness": "scripted", "state": "working", "enqueued_at": "2026-10-02T12:00:00Z",
            "changing_options": [], "commands": [], "depends_on": [], "options": [], "supplied": [], "tools": [], "units": [],
            "message_buffering": false, "thought_buffering": false, "observation": { "availability": "unavailable", "last": null }
        });
        record
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        serde_json::from_value(record).unwrap()
    }

    fn session_output(
        presentation: &Presentation,
        columns: &Columns<kestrel_operator_types::Session>,
        record: &kestrel_operator_types::Session,
    ) -> String {
        said(presentation, columns, record, std::slice::from_ref(record))
    }

    #[test]
    fn session_show_lists_running_tools_with_status_and_start_time() {
        let record = session(
            serde_json::json!({"tools":[{"call_id":"one","title":"read README.md","status":"in_progress","tool_kind":"read","started_at":"2026-09-30T12:00:00Z"}]}),
        );
        let shown = session_output(&Presentation::Delimited, &crate::view::SESSION, &record);
        assert!(shown.contains("read README.md"));
        assert!(shown.contains("in_progress"));
        assert!(shown.contains("2026-09-30T12:00:00Z"));
        let shown = session_output(&Presentation::Human(240), &crate::view::SESSION, &record);
        assert!(shown.contains("read README.md"));
        assert!(shown.contains("in_progress"));
        assert!(shown.contains("2026-09-30T12:00:00Z"));
    }

    #[test]
    fn session_show_lists_running_units_with_kind_and_start_time() {
        let record = session(
            serde_json::json!({"units":[{"id":"task","kind":"background_task","title":"cargo test","started_at":"2026-10-02T12:00:00Z"}]}),
        );
        let shown = session_output(&Presentation::Human(240), &crate::view::SESSION, &record);
        assert!(shown.contains("cargo test"), "{shown}");
        assert!(shown.contains("background_task"), "{shown}");
        assert!(shown.contains("2026-10-02T12:00:00Z"), "{shown}");
    }

    #[test]
    fn session_show_and_list_say_a_session_trails_and_when_its_agent_was_last_active() {
        let record = session(
            serde_json::json!({"state":"trailing","last_activity_at":"2026-10-02T12:00:00Z"}),
        );
        let shown = session_output(&Presentation::Human(240), &crate::view::SESSION, &record);
        assert!(shown.contains("trailing"), "{shown}");
        assert!(shown.contains("2026-10-02T12:00:00Z"), "{shown}");
        let listed = session_output(&Presentation::Delimited, &crate::view::SESSIONS, &record);
        assert!(listed.contains("trailing"), "{listed}");
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

    #[test]
    fn a_streamed_record_is_one_whole_json_line_however_much_it_spans() {
        let mut out = Vec::new();
        let entry = Agent {
            id: 3,
            name: "one\ntwo\tthree",
            model: None,
            tags: Vec::new(),
        };

        record(&mut out, &Presentation::Json(None), &AGENT, &entry).expect("a record");

        let shown = String::from_utf8(out).expect("utf-8");
        assert_eq!(shown.lines().count(), 1, "{shown}");
        assert!(shown.contains("one\\ntwo\\tthree"), "{shown}");
    }
}
