use std::io::{IsTerminal as _, Write};

use anyhow::{Context as _, Result};
use jaq_json::Val;
use serde_json::Value;

use crate::filter::Filter;
use crate::view::View;

static NULL: Value = Value::Null;
pub const ABSENT: &str = "-";
pub const GAP: &str = "  ";

pub enum Presentation {
    Human(usize),
    Delimited,
    /// The complete answer as JSON; `Some` filters it through the embedded jq.
    Json(Option<Filter>),
}

impl Presentation {
    pub fn chosen(json: bool, filter: Option<Filter>) -> Self {
        if let Some(filter) = filter {
            return Presentation::Json(Some(filter));
        }
        if json {
            Presentation::Json(None)
        } else if std::io::stdout().is_terminal() {
            Presentation::Human(columns())
        } else {
            Presentation::Delimited
        }
    }

    /// Whether the invocation asked for the complete response, with or without a filter.
    pub fn json(&self) -> bool {
        matches!(self, Presentation::Json(_))
    }

    pub fn filter(&self) -> Option<&Filter> {
        match self {
            Presentation::Json(filter) => filter.as_ref(),
            _ => None,
        }
    }
}

pub fn show(presentation: &Presentation, view: &View, answer: &Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    written(&mut out, presentation, view, answer)?;
    out.flush().context("writing to standard output")
}

/// The complete answer as JSON, filtered when the invocation asked for it, and the one place a
/// success answer reaches standard output.
pub fn document(out: &mut impl Write, presentation: &Presentation, answer: &Value) -> Result<()> {
    json(out, presentation.filter(), answer)
}

fn written(
    out: &mut impl Write,
    presentation: &Presentation,
    view: &View,
    answer: &Value,
) -> Result<()> {
    if let Presentation::Json(filter) = presentation {
        return json(out, filter.as_ref(), answer);
    }
    let records: Vec<&Value> = match answer {
        Value::Null => Vec::new(),
        Value::Array(records) => records.iter().collect(),
        record => vec![record],
    };
    match presentation {
        Presentation::Human(width) => human(out, view, &records, *width),
        presentation => {
            for record in records {
                writeln!(out, "{}", line(presentation, view, record))?;
            }
            Ok(())
        }
    }
}

fn json(out: &mut impl Write, filter: Option<&Filter>, answer: &Value) -> Result<()> {
    match filter {
        Some(filter) => filtered(out, filter, answer),
        None => {
            writeln!(out, "{answer}")?;
            Ok(())
        }
    }
}

/// A filter's results, where a string prints raw, anything else as JSON, and no results at all is
/// success.
fn filtered(out: &mut impl Write, filter: &Filter, answer: &Value) -> Result<()> {
    filter.each(answer, |value| result(out, &value))
}

fn result(out: &mut impl Write, value: &Val) -> Result<()> {
    match value {
        Val::BStr(bytes) | Val::TStr(bytes) => {
            out.write_all(&bytes[..])?;
            out.write_all(b"\n")?;
        }
        value => writeln!(out, "{value}")?,
    }
    Ok(())
}

/// One record as it arrives, unclipped and unfolded because a streamed entry is prose; a filtered
/// presentation runs the filter first, in [`written`].
pub fn line(presentation: &Presentation, view: &View, record: &Value) -> String {
    match presentation {
        Presentation::Json(_) => record.to_string(),
        Presentation::Delimited => delimited_record(record, view),
        Presentation::Human(_) => view
            .fields()
            .iter()
            .map(|field| rendered(at(record, field)))
            .collect::<Vec<_>>()
            .join(GAP),
    }
}

fn delimited_record(record: &Value, view: &View) -> String {
    let cells: Vec<String> = view
        .fields()
        .iter()
        .map(|field| rendered(at(record, field)))
        .collect();
    delimited(&cells)
}

pub fn delimited(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| one_line(cell))
        .collect::<Vec<_>>()
        .join("\t")
}

pub enum Shape {
    Value,
    Rows,
    Detail,
}

fn human(out: &mut impl Write, view: &View, records: &[&Value], width: usize) -> Result<()> {
    let shape = match view {
        View::Value(_) => Shape::Value,
        View::Rows(_) => Shape::Rows,
        View::Detail(_) => Shape::Detail,
    };
    let labels: Vec<String> = view.fields().iter().map(|field| label(field)).collect();
    let rows: Vec<Vec<String>> = records
        .iter()
        .map(|record| {
            view.fields()
                .iter()
                .map(|field| match (&shape, *field) {
                    (Shape::Detail, "tools") => running(record, field, "status"),
                    (Shape::Detail, "units") => running(record, field, "kind"),
                    (Shape::Detail, "depends_on") => names(record, field),
                    _ => rendered(at(record, field)),
                })
                .collect()
        })
        .collect();
    table(out, &shape, &labels, &rows, width)
}

/// A cell arrives as it reads, before it is fitted to a line or a width.
pub fn table(
    out: &mut impl Write,
    shape: &Shape,
    labels: &[String],
    rows: &[Vec<String>],
    width: usize,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    match shape {
        Shape::Value => {
            for cell in rows.iter().flatten() {
                writeln!(out, "{}", truncated(cell, width))?;
            }
        }
        Shape::Rows => {
            let mut table = vec![labels.to_vec()];
            table.extend(
                rows.iter()
                    .map(|row| row.iter().map(|cell| one_line(cell)).collect()),
            );
            let widths = widest(&table);
            for row in &table {
                writeln!(out, "{}", truncated(&padded(row, &widths), width))?;
            }
        }
        Shape::Detail => {
            let column = labels
                .iter()
                .map(|label| label.chars().count())
                .max()
                .unwrap_or_default();
            for (place, row) in rows.iter().enumerate() {
                if place > 0 {
                    writeln!(out)?;
                }
                for (label, value) in labels.iter().zip(row) {
                    let mut lines = value.lines();
                    let first = lines.next().unwrap_or_default();
                    writeln!(
                        out,
                        "{}",
                        truncated(&format!("{label:column$}{GAP}{first}"), width)
                    )?;
                    for line in lines {
                        writeln!(
                            out,
                            "{}",
                            truncated(&format!("{:column$}{GAP}{line}", ""), width)
                        )?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// A path that reaches through a null holds no value, which is not the same as naming a field
/// the answer does not have.
fn at<'a>(record: &'a Value, path: &str) -> Option<&'a Value> {
    let mut value = record;
    for key in path.split('.') {
        if value.is_null() {
            return Some(&NULL);
        }
        value = value.get(key)?;
    }

    Some(value)
}

fn running(record: &Value, field: &str, beside: &str) -> String {
    at(record, field)
        .and_then(Value::as_array)
        .map(|running| {
            running
                .iter()
                .map(|one| {
                    format!(
                        "{}\n{}  {}",
                        rendered(one.get("title")),
                        rendered(one.get(beside)),
                        rendered(one.get("started_at"))
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn names(record: &Value, field: &str) -> String {
    let names: Vec<Value> = at(record, field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|named| named.get("name").cloned())
        .collect();
    rendered(Some(&Value::Array(names)))
}

pub(crate) fn rendered(value: Option<&Value>) -> String {
    match value.unwrap_or(&NULL) {
        Value::Null => ABSENT.to_owned(),
        Value::Bool(yes) => yes.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) if items.is_empty() => ABSENT.to_owned(),
        Value::Array(items) => items
            .iter()
            .map(|item| rendered(Some(item)))
            .collect::<Vec<_>>()
            .join(","),
        object => object.to_string(),
    }
}

/// A record is a line, so a brief that spans several of them is escaped rather than splitting it.
fn one_line(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn label(path: &str) -> String {
    path.rsplit('.').next().unwrap_or(path).replace('_', " ")
}

fn widest(table: &[Vec<String>]) -> Vec<usize> {
    (0..table.first().map_or(0, Vec::len))
        .map(|column| {
            table
                .iter()
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or_default()
        })
        .collect()
}

fn padded(row: &[String], widths: &[usize]) -> String {
    row.iter()
        .zip(widths)
        .enumerate()
        .map(|(place, (cell, width))| {
            if place + 1 == row.len() {
                cell.clone()
            } else {
                format!("{cell:width$}")
            }
        })
        .collect::<Vec<_>>()
        .join(GAP)
}

fn truncated(line: &str, width: usize) -> String {
    if line.chars().count() <= width {
        return line.to_owned();
    }
    if width == 0 {
        return String::new();
    }

    let mut cut: String = line.chars().take(width - 1).collect();
    cut.push('…');
    cut
}

fn columns() -> usize {
    terminal_size::terminal_size().map_or(usize::MAX, |(width, _)| usize::from(width.0))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const AGENTS: View = View::Rows(&["name", "harness", "model"]);

    fn shown(presentation: &Presentation, view: &View, answer: &Value) -> Vec<String> {
        answer
            .as_array()
            .expect("an array of records")
            .iter()
            .map(|record| line(presentation, view, record))
            .collect()
    }

    #[test]
    fn a_pipe_gets_every_field_of_every_record_joined_by_tabs() {
        let agents = json!([{ "name": "builder", "harness": "opencode", "model": null }]);

        assert_eq!(
            shown(&Presentation::Delimited, &AGENTS, &agents),
            ["builder\topencode\t-"]
        );
    }

    #[test]
    fn a_record_spanning_lines_is_still_one_line_of_a_pipe() {
        let triggers = json!([{ "name": "nightly", "harness": "a\nb", "model": null }]);

        assert_eq!(
            shown(&Presentation::Delimited, &AGENTS, &triggers),
            ["nightly\ta\\nb\t-"]
        );
    }

    fn written_as_json(answer: &Value) -> String {
        let mut out = Vec::new();
        written(&mut out, &Presentation::Json(None), &AGENTS, answer).expect("written");
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn json_emits_the_whole_record_and_not_the_commands_fields() {
        let record = json!({ "id": "01", "name": "builder", "harness": "opencode", "extra": { "deep": [1, 2] } });

        let written = written_as_json(&record);

        assert_eq!(
            serde_json::from_str::<Value>(&written).expect("json"),
            record
        );
    }

    /// One document, so `jq` reads the collection rather than a stream of its members.
    #[test]
    fn json_emits_a_collection_as_one_array_and_nothing_as_an_empty_one() {
        let agents = json!([{ "name": "builder" }, { "name": "reviewer" }]);

        assert_eq!(written_as_json(&agents).lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(&written_as_json(&agents)).expect("json"),
            agents
        );
        assert_eq!(written_as_json(&json!([])), "[]\n");
    }

    #[test]
    fn a_streamed_record_is_one_whole_json_line_however_much_it_spans() {
        let entry = json!({ "seq": 3, "entry": { "text": "one\ntwo\tthree" } });

        let shown = line(
            &Presentation::Json(None),
            &View::Rows(&["seq", "entry"]),
            &entry,
        );

        assert!(!shown.contains('\n'), "{shown}");
        assert_eq!(serde_json::from_str::<Value>(&shown).expect("json"), entry);
    }

    #[test]
    fn json_is_never_clipped_to_a_terminal() {
        let long = "x".repeat(500);

        let written = written_as_json(&json!([{ "name": long }]));

        assert!(written.contains(&long));
        assert!(!written.contains('…'));
    }

    #[test]
    fn a_terminal_gets_columns_that_line_up_under_their_labels() {
        let agents = json!([
            { "name": "builder", "harness": "opencode", "model": "claude-opus-5" },
            { "name": "reviewer", "harness": "acp", "model": null },
        ]);
        let mut written = Vec::new();

        human(
            &mut written,
            &AGENTS,
            &agents
                .as_array()
                .expect("records")
                .iter()
                .collect::<Vec<_>>(),
            usize::MAX,
        )
        .expect("a table");

        assert_eq!(
            String::from_utf8(written).expect("utf-8"),
            "name      harness   model\n\
             builder   opencode  claude-opus-5\n\
             reviewer  acp       -\n"
        );
    }

    #[test]
    fn a_terminal_gets_a_line_no_wider_than_it_is() {
        let agents =
            json!([{ "name": "builder", "harness": "opencode", "model": "claude-opus-5" }]);
        let mut written = Vec::new();

        human(
            &mut written,
            &AGENTS,
            &agents
                .as_array()
                .expect("records")
                .iter()
                .collect::<Vec<_>>(),
            12,
        )
        .expect("a table");

        for line in String::from_utf8(written).expect("utf-8").lines() {
            assert!(line.chars().count() <= 12, "{line:?} is wider than 12");
        }
    }
}
