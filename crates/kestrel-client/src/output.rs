use std::io::{IsTerminal as _, Write};

use anyhow::Result;
use jaq_json::Val;
use serde_json::Value;

use crate::filter::Filter;

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

/// The complete answer as JSON, filtered when the invocation asked for it, and the one place a
/// success answer reaches standard output.
pub fn document(out: &mut impl Write, presentation: &Presentation, answer: &Value) -> Result<()> {
    json(out, presentation.filter(), answer)
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

pub(crate) fn rendered(value: Option<&Value>) -> String {
    match value.unwrap_or(&Value::Null) {
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

    fn document_as(presentation: &Presentation, answer: &Value) -> String {
        let mut out = Vec::new();
        document(&mut out, presentation, answer).expect("written");
        String::from_utf8(out).expect("utf-8")
    }

    fn rows(shape: &Shape, labels: &[&str], rows: &[&[&str]], width: usize) -> String {
        let labels: Vec<String> = labels.iter().map(|label| (*label).to_owned()).collect();
        let rows: Vec<Vec<String>> = rows
            .iter()
            .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
            .collect();
        let mut out = Vec::new();
        table(&mut out, shape, &labels, &rows, width).expect("a table");
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn json_emits_the_whole_record_and_not_the_columns() {
        let record = json!({ "id": "01", "name": "builder", "harness": "opencode", "extra": { "deep": [1, 2] } });

        assert_eq!(
            serde_json::from_str::<Value>(&document_as(&Presentation::Json(None), &record))
                .expect("json"),
            record
        );
    }

    /// One document, so `jq` reads the collection rather than a stream of its members.
    #[test]
    fn json_emits_a_collection_as_one_array_and_nothing_as_an_empty_one() {
        let agents = json!([{ "name": "builder" }, { "name": "reviewer" }]);

        assert_eq!(
            document_as(&Presentation::Json(None), &agents)
                .lines()
                .count(),
            1
        );
        assert_eq!(
            serde_json::from_str::<Value>(&document_as(&Presentation::Json(None), &agents))
                .expect("json"),
            agents
        );
        assert_eq!(document_as(&Presentation::Json(None), &json!([])), "[]\n");
    }

    #[test]
    fn json_is_never_clipped_to_a_terminal() {
        let long = "x".repeat(500);

        let written = document_as(&Presentation::Json(None), &json!([{ "name": long }]));

        assert!(written.contains(&long));
        assert!(!written.contains('…'));
    }

    #[test]
    fn a_filtered_answer_prints_a_string_raw_and_anything_else_as_json() {
        let agents = json!([{ "name": "builder" }, { "name": "reviewer" }]);
        let filtered = |expression: &str| {
            let filter = Filter::compile(expression).expect("the filter should compile");
            document_as(&Presentation::Json(Some(filter)), &agents)
        };

        assert_eq!(filtered(".[] | .name"), "builder\nreviewer\n");
        assert_eq!(filtered("map(.name)"), "[\"builder\",\"reviewer\"]\n");
        assert_eq!(filtered(r#".[] | select(.name == "nobody")"#), "");
    }

    #[test]
    fn a_record_spanning_lines_is_still_one_line_of_a_pipe() {
        assert_eq!(
            delimited(&[
                "nightly".to_owned(),
                "a\nb".to_owned(),
                "c\td\re\\f".to_owned()
            ]),
            "nightly\ta\\nb\tc\\td\\re\\\\f"
        );
    }

    #[test]
    fn a_terminal_gets_columns_that_line_up_under_their_labels() {
        let written = rows(
            &Shape::Rows,
            &["name", "harness", "model"],
            &[
                &["builder", "opencode", "claude-opus-5"],
                &["reviewer", "acp", "-"],
            ],
            usize::MAX,
        );

        assert_eq!(
            written,
            "name      harness   model\n\
             builder   opencode  claude-opus-5\n\
             reviewer  acp       -\n"
        );
    }

    #[test]
    fn a_terminal_gets_a_line_no_wider_than_it_is() {
        let written = rows(
            &Shape::Rows,
            &["name", "harness", "model"],
            &[&["builder", "opencode", "claude-opus-5"]],
            12,
        );

        for line in written.lines() {
            assert!(line.chars().count() <= 12, "{line:?} is wider than 12");
        }
    }

    #[test]
    fn a_lone_value_is_written_alone_and_a_detail_pairs_labels_with_values() {
        assert_eq!(
            rows(&Shape::Value, &["status"], &[&["stopped"]], usize::MAX),
            "stopped\n"
        );
        assert_eq!(
            rows(
                &Shape::Detail,
                &["id", "name"],
                &[&["01", "builder"], &["02", "reviewer"]],
                usize::MAX
            ),
            "id    01\nname  builder\n\nid    02\nname  reviewer\n"
        );
    }

    #[test]
    fn nothing_is_written_for_no_records() {
        assert_eq!(rows(&Shape::Rows, &["name"], &[], usize::MAX), "");
    }

    #[test]
    fn a_missing_field_reads_as_absent_and_an_empty_list_the_same() {
        assert_eq!(rendered(None), ABSENT);
        assert_eq!(rendered(Some(&Value::Null)), ABSENT);
        assert_eq!(rendered(Some(&json!([]))), ABSENT);
        assert_eq!(rendered(Some(&json!([1, 2]))), "1,2");
        assert_eq!(rendered(Some(&json!("x"))), "x");
    }
}
