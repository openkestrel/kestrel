//! The operator document is authored as one fragment per area in `openapi/operator/`, so two
//! changes in different areas edit different files; this is the one place they become a document.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};

/// Reads every fragment in `fragments` and merges them into the whole document.
///
/// # Errors
///
/// When a fragment is unreadable or no JSON object, or two fragments define the same path, schema
/// or other value.
pub fn read(fragments: &Path) -> io::Result<Value> {
    let mut files = fs::read_dir(fragments)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    files.retain(|file| {
        file.extension()
            .is_some_and(|extension| extension == "json")
    });
    files.sort();

    let mut document = Map::new();
    for file in files {
        let Value::Object(fragment) = serde_json::from_str(&fs::read_to_string(&file)?)? else {
            return Err(io::Error::other(format!(
                "{} is not a JSON object",
                file.display()
            )));
        };
        merge(&mut document, fragment, "").map_err(|twice| {
            io::Error::other(format!("{} defines {twice} again", file.display()))
        })?;
    }
    Ok(Value::Object(document))
}

fn merge(
    whole: &mut Map<String, Value>,
    fragment: Map<String, Value>,
    at: &str,
) -> Result<(), String> {
    for (key, value) in fragment {
        let at = format!("{at}/{key}");
        match (whole.get_mut(&key), value) {
            (None, value) => {
                whole.insert(key, value);
            }
            (Some(Value::Object(held)), Value::Object(more)) => merge(held, more, &at)?,
            (Some(_), _) => return Err(at),
        }
    }
    Ok(())
}
