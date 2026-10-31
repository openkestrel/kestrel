use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};

/// Reads the OpenAPI document rooted at `root` and the files its `$ref`s name as one document:
/// every other file a reference reaches outside the root's `paths` is a map of named schemas, which
/// all join the root's own `components.schemas` under their names. A JSON file in the directory
/// named for the root (`operator/` beside `operator.json`) that no reference reaches is refused.
pub fn bundle(root: &Path) -> io::Result<Value> {
    let root = fs::canonicalize(root)?;
    let mut bundle = Bundle {
        files: HashMap::new(),
        hoisted: HashSet::new(),
        owners: HashMap::new(),
        schemas: Map::new(),
        root: root.clone(),
    };
    let document = bundle.load(&root)?;
    let Value::Object(document) = document else {
        return Err(refused(&root, "is not an object"));
    };
    let mut bundled = Map::new();
    for (key, value) in document {
        let value = if key == "paths" {
            bundle.paths(&value)?
        } else {
            bundle.walk(&value, &root)?
        };
        bundled.insert(key, value);
    }
    let components = bundled
        .entry("components")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| refused(&root, "has components that are not an object"))?;
    let schemas = components
        .entry("schemas")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| refused(&root, "has schemas that are not an object"))?;
    for (name, schema) in bundle.schemas {
        if schemas.insert(name.clone(), schema).is_some() {
            return Err(refused(
                &bundle.owners[&name],
                &format!("names the schema {name}, which the root already names"),
            ));
        }
    }
    if let Some(stray) = json_files(&root.with_extension(""))?
        .into_iter()
        .find(|file| !bundle.files.contains_key(file))
    {
        return Err(refused(
            &stray,
            "is beside the root, but no reference reaches it",
        ));
    }
    Ok(Value::Object(bundled))
}

fn json_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        entries => entries?,
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = fs::canonicalize(entry?.path())?;
        if path.is_dir() {
            files.extend(json_files(&path)?);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
    Ok(files)
}

struct Bundle {
    root: PathBuf,
    files: HashMap<PathBuf, Value>,
    hoisted: HashSet<PathBuf>,
    owners: HashMap<String, PathBuf>,
    schemas: Map<String, Value>,
}

impl Bundle {
    fn load(&mut self, file: &Path) -> io::Result<Value> {
        if let Some(value) = self.files.get(file) {
            return Ok(value.clone());
        }
        let value: Value = serde_json::from_str(&fs::read_to_string(file)?)
            .map_err(|error| refused(file, &error.to_string()))?;
        self.files.insert(file.to_path_buf(), value.clone());
        Ok(value)
    }

    fn paths(&mut self, paths: &Value) -> io::Result<Value> {
        let paths = paths
            .as_object()
            .ok_or_else(|| refused(&self.root, "has paths that are not an object"))?;
        let mut bundled = Map::new();
        for (path, item) in paths {
            let item = match item.get("$ref").and_then(Value::as_str) {
                Some(reference) => {
                    let (file, pointer) = self.target(reference, &self.root.clone())?;
                    let target = self
                        .load(&file)?
                        .pointer(pointer)
                        .cloned()
                        .ok_or_else(|| refused(&file, &format!("has nothing at {pointer}")))?;
                    self.walk(&target, &file)?
                }
                None => self.walk(item, &self.root.clone())?,
            };
            bundled.insert(path.clone(), item);
        }
        Ok(Value::Object(bundled))
    }

    fn walk(&mut self, value: &Value, base: &Path) -> io::Result<Value> {
        match value {
            Value::Array(items) => items
                .iter()
                .map(|item| self.walk(item, base))
                .collect::<io::Result<_>>()
                .map(Value::Array),
            Value::Object(members) => {
                let mut walked = Map::new();
                for (key, member) in members {
                    let member = match (key.as_str(), member) {
                        ("$ref", Value::String(reference)) => {
                            Value::String(self.reference(reference, base)?)
                        }
                        _ => self.walk(member, base)?,
                    };
                    walked.insert(key.clone(), member);
                }
                Ok(Value::Object(walked))
            }
            _ => Ok(value.clone()),
        }
    }

    fn reference(&mut self, reference: &str, base: &Path) -> io::Result<String> {
        let (file, pointer) = self.target(reference, base)?;
        if file == self.root {
            return Ok(format!("#{pointer}"));
        }
        let name = pointer
            .strip_prefix('/')
            .filter(|name| !name.contains('/'))
            .ok_or_else(|| refused(base, &format!("refers to {reference}, not a named schema")))?;
        self.hoist(&file)?;
        if self.owners.get(name) != Some(&file) {
            return Err(refused(
                base,
                &format!("refers to {reference}, which is not there"),
            ));
        }
        Ok(format!("#/components/schemas/{name}"))
    }

    fn target<'a>(&self, reference: &'a str, base: &Path) -> io::Result<(PathBuf, &'a str)> {
        let (file, pointer) = reference.split_once('#').unwrap_or((reference, ""));
        if file.is_empty() {
            return Ok((base.to_path_buf(), pointer));
        }
        let directory = base.parent().unwrap_or(Path::new(""));
        let file = fs::canonicalize(directory.join(file))
            .map_err(|error| refused(base, &format!("refers to {reference}: {error}")))?;
        Ok((file, pointer))
    }

    fn hoist(&mut self, file: &Path) -> io::Result<()> {
        if !self.hoisted.insert(file.to_path_buf()) {
            return Ok(());
        }
        let Value::Object(schemas) = self.load(file)? else {
            return Err(refused(file, "is not a map of named schemas"));
        };
        for name in schemas.keys() {
            if let Some(owner) = self.owners.insert(name.clone(), file.to_path_buf()) {
                return Err(refused(
                    file,
                    &format!(
                        "names the schema {name}, which {} already names",
                        owner.display()
                    ),
                ));
            }
        }
        for (name, schema) in &schemas {
            let schema = self.walk(schema, file)?;
            self.schemas.insert(name.clone(), schema);
        }
        Ok(())
    }
}

fn refused(file: &Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} {reason}", file.display()),
    )
}
