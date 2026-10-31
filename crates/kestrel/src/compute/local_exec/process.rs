//! The handle to a supervisor that this process started, and the process group it leads.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Child;

use super::super::{Exited, Supervising};

pub(super) struct Process {
    pub(super) child: Child,
    pub(super) marker: PathBuf,
    #[cfg(unix)]
    pub(super) pgid: i32,
}

impl Supervising for Process {
    fn status(&mut self) -> io::Result<Option<Exited>> {
        Ok(self.child.try_wait()?.map(Exited::from))
    }

    fn stop(&mut self) -> io::Result<()> {
        // Darwin refuses to signal a group whose only member left is its unreaped leader, which
        // is what `stop_named` reaping it first leaves behind.
        #[cfg(unix)]
        if let Err(error) = killed(self.pgid) {
            self.child.kill().map_err(|_| error)?;
        }
        #[cfg(not(unix))]
        self.child.kill()?;

        self.child.wait()?;
        let _ = fs::remove_file(&self.marker);
        Ok(())
    }
}

/// Every process in the tree, not only the one the supervisor is.
#[cfg(unix)]
pub(super) fn killed(pgid: i32) -> io::Result<()> {
    #[allow(unsafe_code)]
    let killed = unsafe { libc::killpg(pgid, libc::SIGKILL) };
    if killed == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }

    Ok(())
}
