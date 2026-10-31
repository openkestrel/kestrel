//! An Instance as a directory on this machine: what a Session may run in it, read and write, and
//! the supervisor whose process group is its tree.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[cfg(unix)]
use std::os::fd::AsRawFd as _;
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

use super::markers::{stop_processes, supervisors};
use super::process::Process;
#[cfg(unix)]
use super::process::killed;
use super::removed;

use super::super::{Provisioned, Streaming, Supervisor, Terminal};

pub(super) struct Directory {
    pub(super) supervisor: PathBuf,
    pub(super) lifecycle: Arc<Mutex<HashSet<String>>>,
    pub(super) path: Option<std::ffi::OsString>,
    pub(super) name: String,
    pub(super) root: PathBuf,
    pub(super) home: PathBuf,
}

impl Directory {
    fn at(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }
}

impl Provisioned for Directory {
    fn terminal(&mut self, argv: &[&str]) -> anyhow::Result<Terminal> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::other("nothing to run"))?;
        let lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| io::Error::other("instance lock poisoned"))?;
        if lifecycle.contains(&self.name) {
            return Err(io::Error::other("instance was destroyed").into());
        }
        let mut command = Command::new(program);
        command.args(args);
        command.current_dir(&self.root);
        command.env_clear();
        command.env("HOME", &self.home);
        command.env(
            "PATH",
            self.path
                .clone()
                .unwrap_or_else(|| std::env::var_os("PATH").unwrap_or_default()),
        );
        command.env("TERM", "xterm-256color");
        command.env("BROWSER", "/bin/true");
        Terminal::spawn(command, Some(&supervisors(&self.home)))
    }

    fn exec(&mut self, command: &[&str]) -> io::Result<Streaming> {
        let (program, arguments) = command
            .split_first()
            .ok_or_else(|| io::Error::other("nothing to exec in the instance"))?;

        let child = Command::new(program)
            .args(arguments)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(Streaming { child })
    }

    fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(self.at(path))
    }

    fn read_home_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(self.home.join(path))
    }

    fn write_file(&mut self, path: &str, contents: &[u8]) -> io::Result<()> {
        let path = self.at(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, contents)
    }

    fn supervise(&mut self, variables: &[(&str, &str)]) -> io::Result<Supervisor> {
        let lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| io::Error::other("instance lock poisoned"))?;
        if lifecycle.contains(&self.name) || !self.root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "instance was destroyed",
            ));
        }
        let mut command = Command::new(&self.supervisor);
        fs::create_dir_all(&self.home)?;
        let markers = supervisors(&self.home);
        fs::create_dir_all(&markers)?;
        let pending = markers.join(format!(".{}", uuid::Uuid::now_v7()));
        let marker_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        #[cfg(unix)]
        {
            #[allow(unsafe_code)]
            let locked = unsafe { libc::flock(marker_file.as_raw_fd(), libc::LOCK_EX) };
            if locked == -1 {
                let error = io::Error::last_os_error();
                let _ = fs::remove_file(&pending);
                return Err(error);
            }
        }
        command
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .envs(variables.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        {
            let marker_fd = marker_file.as_raw_fd();
            // `setsid` makes this process its own process-group leader, so every
            // child it forks inherits the same group and `killpg` reaches all of them.
            #[allow(unsafe_code)]
            unsafe {
                command.pre_exec(move || {
                    if libc::setsid() == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::fcntl(marker_fd, libc::F_SETFD, 0) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&pending);
                return Err(error);
            }
        };
        let marker = markers.join(child.id().to_string());
        if let Err(error) = fs::rename(&pending, &marker) {
            #[cfg(unix)]
            let _ = killed(child.id() as i32);
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&pending);
            return Err(error);
        }
        drop(marker_file);

        Ok(Supervisor {
            name: format!("local-exec/{}@{}", child.id(), self.name),
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            supervising: Box::new(Process {
                marker,
                #[cfg(unix)]
                pgid: child.id() as i32,
                child,
            }),
        })
    }

    fn destroy(&mut self) -> io::Result<()> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| io::Error::other("instance lock poisoned"))?;
        lifecycle.insert(self.name.clone());
        stop_processes(&self.home)?;
        removed(&self.home)?;
        removed(&self.root)
    }
}
