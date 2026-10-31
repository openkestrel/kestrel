//! The marker files that outlive a supervisor handle, and what stops the tree each one names.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::fd::AsRawFd as _;

use super::home;
#[cfg(unix)]
use super::process::killed;

pub(super) fn supervisors(home: &Path) -> PathBuf {
    home.join(".kestrel-supervisors")
}

pub(super) fn stop_processes(home: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(supervisors(home)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        entries => entries?,
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            if marker_is_locked(&entry.path())? {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "supervisor is starting",
                ));
            }
            let _ = fs::remove_file(entry.path());
            continue;
        }
        let pid: i32 = entry
            .file_name()
            .to_string_lossy()
            .parse()
            .map_err(|error| io::Error::other(format!("invalid supervisor marker: {error}")))?;
        stop_marker(&entry.path(), pid)?;
    }
    Ok(())
}

fn stop_marker(marker: &Path, pid: i32) -> io::Result<()> {
    if marker.exists() && marker_is_locked(marker)? {
        #[cfg(unix)]
        if let Err(error) = killed(pid)
            && marker_is_locked(marker)?
        {
            return Err(error);
        }
        #[cfg(not(unix))]
        let _ = pid;
    }
    match fs::remove_file(marker) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

#[cfg(unix)]
fn marker_is_locked(marker: &Path) -> io::Result<bool> {
    let file = match OpenOptions::new().read(true).open(marker) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        opened => opened?,
    };
    #[allow(unsafe_code)]
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if locked == 0 {
        Ok(false)
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(true)
        } else {
            Err(error)
        }
    }
}

#[cfg(not(unix))]
fn marker_is_locked(_: &Path) -> io::Result<bool> {
    Ok(true)
}

pub(crate) fn stop_named(supervisor: &str) -> io::Result<()> {
    let named = supervisor
        .strip_prefix("local-exec/")
        .ok_or_else(|| io::Error::other(format!("{supervisor} is not a local supervisor")))?;
    let (pid, instance) = named
        .split_once('@')
        .ok_or_else(|| io::Error::other(format!("{supervisor} has no instance")))?;
    if instance.is_empty() || instance.contains(['/', '\\']) || instance == ".." {
        return Err(io::Error::other(format!(
            "{supervisor} has no valid instance"
        )));
    }
    let pid: i32 = pid
        .parse()
        .map_err(|error| io::Error::other(format!("{supervisor} has no process id: {error}")))?;

    stop_marker(&supervisors(&home(instance)).join(pid.to_string()), pid)
}
