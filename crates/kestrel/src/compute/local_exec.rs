//! The escape hatch that exists whether or not it is planned (ADR-0005), and the Instance the
//! primary test seam provisions: a directory, and its supervisor's process tree.

mod directory;
mod markers;
mod process;

#[cfg(all(test, unix))]
mod tests;

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::{IdleHint, Instance};

use self::directory::Directory;

pub(super) use self::markers::stop_named;

#[derive(Debug, Clone)]
pub struct LocalExec {
    supervisor: PathBuf,
    lifecycle: Arc<Mutex<HashSet<String>>>,
    path: Option<std::ffi::OsString>,
}

impl LocalExec {
    pub(super) fn idle_hint(&self, _hint: &IdleHint) -> io::Result<()> {
        Ok(())
    }

    pub fn running(supervisor: impl Into<PathBuf>) -> Self {
        Self {
            supervisor: supervisor.into(),
            lifecycle: Arc::new(Mutex::new(HashSet::new())),
            path: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub(super) fn provision(&self, holder: &str) -> io::Result<Instance> {
        let lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| io::Error::other("instance lock poisoned"))?;
        let name = super::named_after(&holder);
        if lifecycle.contains(&name) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "instance was destroyed",
            ));
        }
        fs::create_dir_all(within(&name))?;
        fs::create_dir_all(home(&name))?;

        Ok(self.instance(name))
    }

    pub(super) fn resume(&self, instance: &str) -> io::Result<Option<Instance>> {
        let name = named(instance)?;
        if !within(name).is_dir() {
            return Ok(None);
        }

        Ok(Some(self.instance(name.to_owned())))
    }

    pub(super) fn destroy_named(&self, instance: &str) -> io::Result<()> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| io::Error::other("instance lock poisoned"))?;
        let name = named(instance)?;
        lifecycle.insert(name.to_owned());
        markers::stop_processes(&home(name))?;
        removed(&home(name))?;
        removed(&within(name))
    }

    fn instance(&self, name: String) -> Instance {
        Instance {
            provisioned: Box::new(Directory {
                supervisor: self.supervisor.clone(),
                path: self.path.clone(),
                lifecycle: self.lifecycle.clone(),
                name: name.clone(),
                root: within(&name),
                home: home(&name),
            }),
            name: format!("local-exec/{name}"),
        }
    }
}

fn within(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// The agent's home, apart from the operator's: a Subscription Profile's files are written
/// beneath it and removed again, which in the operator's own home would take their login with it.
fn home(name: &str) -> PathBuf {
    within(&format!("{name}.home"))
}

fn named(instance: &str) -> io::Result<&str> {
    instance
        .strip_prefix("local-exec/")
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']) && *name != "..")
        .ok_or_else(|| io::Error::other(format!("{instance} is not a local instance")))
}

fn removed(root: &Path) -> io::Result<()> {
    match fs::remove_dir_all(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}
