use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::Result;
use tokio::sync::mpsc;

pub struct Terminal {
    child: Child,
    input: File,
    marker: Option<PathBuf>,
    pub output: mpsc::Receiver<Vec<u8>>,
}

impl Terminal {
    #[cfg(unix)]
    pub(super) fn spawn(mut command: Command, markers: Option<&Path>) -> Result<Self> {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};
        use std::os::unix::process::CommandExt as _;

        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 40,
            ws_col: 240,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        #[allow(unsafe_code)]
        let opened = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if opened == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        #[allow(unsafe_code)]
        let (input, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        #[allow(unsafe_code)]
        if unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut pending = None;
        let marker_file = if let Some(markers) = markers {
            std::fs::create_dir_all(markers)?;
            let path = markers.join(format!(".{}", uuid::Uuid::now_v7()));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            #[allow(unsafe_code)]
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == -1 {
                let error = std::io::Error::last_os_error();
                let _ = std::fs::remove_file(path);
                return Err(error.into());
            }
            pending = Some(path);
            Some(file)
        } else {
            None
        };
        let marker_fd = marker_file.as_ref().map(|file| file.as_raw_fd());
        command
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        // The slave becomes the controlling terminal and the cleanup lock survives exec.
        #[allow(unsafe_code)]
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some(fd) = marker_fd
                    && libc::fcntl(fd, libc::F_SETFD, 0) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn();
        drop(command);
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                if let Some(path) = pending {
                    let _ = std::fs::remove_file(path);
                }
                return Err(error.into());
            }
        };
        let mut terminal = Self {
            child,
            input,
            marker: pending,
            output: mpsc::channel(1).1,
        };
        if let Some(path) = &terminal.marker {
            let marker = path
                .parent()
                .expect("marker directory")
                .join(terminal.child.id().to_string());
            std::fs::rename(path, &marker)?;
            terminal.marker = Some(marker);
        }
        drop(marker_file);
        let mut reader = terminal.input.try_clone()?;
        let (send, output) = mpsc::channel(16);
        std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || send.blocking_send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        terminal.output = output;
        Ok(terminal)
    }

    #[cfg(not(unix))]
    pub(super) fn spawn(_: Command, _: Option<&Path>) -> Result<Self> {
        anyhow::bail!("terminal sign-in requires a Unix control plane")
    }

    pub fn write(&mut self, answer: &[u8]) -> Result<()> {
        self.input.write_all(answer)?;
        self.input.flush()?;
        Ok(())
    }

    pub fn exited(&mut self) -> Result<Option<bool>> {
        Ok(self.child.try_wait()?.map(|status| status.success()))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            #[allow(unsafe_code)]
            unsafe {
                libc::killpg(self.child.id() as i32, libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(path) = &self.marker {
            let _ = std::fs::remove_file(path);
        }
    }
}
