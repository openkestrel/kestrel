//! The one port that is driven twice (ADR-0005): the Docker daemon, and a local process tree.

mod docker;
mod local_exec;
mod terminal;
pub use terminal::Terminal;

use std::fmt;
use std::io;
use std::process::{Child, ChildStderr, ChildStdout, ExitStatus};

pub use docker::Docker;
pub use local_exec::LocalExec;

use jiff::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleHint {
    pub instance: String,
    pub idle_since: Timestamp,
    pub archive_deadline: Timestamp,
}

/// What a driver does once it has provisioned, and the whole of it. An inbound address would
/// split the eight deployment targets, so no driver offers one.
pub trait Provisioned: Send {
    fn terminal(&mut self, command: &[&str]) -> anyhow::Result<Terminal>;
    fn exec(&mut self, command: &[&str]) -> io::Result<Streaming>;
    fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>>;
    fn read_home_file(&mut self, path: &str) -> io::Result<Vec<u8>>;
    fn write_file(&mut self, path: &str, contents: &[u8]) -> io::Result<()>;
    fn supervise(&mut self, variables: &[(&str, &str)]) -> io::Result<Supervisor>;
    fn destroy(&mut self) -> io::Result<()>;
}

pub trait Supervising: Send {
    fn status(&mut self) -> io::Result<Option<Exited>>;
    fn stop(&mut self) -> io::Result<()>;
}

/// The sixth operation, and the only place either driver is named: which one executes a Session is
/// read from configuration once, never decided where a Session is executed.
#[derive(Debug, Clone)]
pub enum Driver {
    Docker(Docker),
    LocalExec(LocalExec),
}

impl Driver {
    /// Advisory only: ignoring this changes cost, never correctness (ADR-0018).
    pub fn idle_hint(&self, hint: &IdleHint) -> io::Result<()> {
        match self {
            Driver::Docker(docker) => docker.idle_hint(hint),
            Driver::LocalExec(local_exec) => local_exec.idle_hint(hint),
        }
    }

    /// What this driver names Instances under, as `<driver>/<instance>`.
    pub const fn name(&self) -> &'static str {
        match self {
            Driver::Docker(_) => "docker",
            Driver::LocalExec(_) => "local-exec",
        }
    }

    pub fn provision(&self, holder: impl fmt::Display) -> io::Result<Instance> {
        let holder = holder.to_string();
        match self {
            Driver::Docker(docker) => docker.provision(&holder),
            Driver::LocalExec(local_exec) => local_exec.provision(&holder),
        }
    }

    pub fn provision_until(
        &self,
        holder: &str,
        deadline: std::time::Instant,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> io::Result<Instance> {
        match self {
            Driver::Docker(docker) => docker.provision_until(holder, deadline, cancel),
            Driver::LocalExec(local) => local.provision(holder),
        }
    }

    pub fn destroy_until(&self, instance: &str, deadline: std::time::Instant) -> io::Result<()> {
        match self {
            Driver::Docker(_) => docker::destroy_until(instance, deadline),
            Driver::LocalExec(local) => local.destroy_named(instance),
        }
    }

    /// What `provision` names the Instance it makes for `holder`, which finds one whose handle is gone.
    pub fn instance_for(&self, holder: impl fmt::Display) -> String {
        format!("{}/{}", self.name(), named_after(&holder))
    }

    /// `None` when the Instance is gone, and with it whatever it held.
    pub fn resume(&self, instance: &str) -> io::Result<Option<Instance>> {
        match self {
            Driver::Docker(docker) => docker.resume(instance),
            Driver::LocalExec(local_exec) => local_exec.resume(instance),
        }
    }

    pub fn destroy_named(&self, instance: &str) -> io::Result<()> {
        match self {
            Driver::Docker(_) => docker::destroy_named(instance),
            Driver::LocalExec(local_exec) => local_exec.destroy_named(instance),
        }
    }

    /// Stops a supervisor this process no longer holds, which a restart leaves behind.
    pub fn stop_named(&self, supervisor: &str) -> io::Result<()> {
        match self {
            Driver::Docker(_) => docker::stop_named(supervisor),
            Driver::LocalExec(_) => local_exec::stop_named(supervisor),
        }
    }
}

fn named_after(holder: &dyn fmt::Display) -> String {
    format!("kestrel-{holder}")
}

/// Outlives this handle: dropping one leaves the Instance where it is, for the next Session to
/// resume.
pub struct Instance {
    name: String,
    provisioned: Box<dyn Provisioned>,
}

impl Instance {
    /// `<driver>/<instance>`, which is what a Session records having executed on.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Relative to the Instance's root, as every path either driver takes is.
    pub fn exec(&mut self, command: &[&str]) -> io::Result<Streaming> {
        self.provisioned.exec(command)
    }

    pub fn terminal(&mut self, command: &[&str]) -> anyhow::Result<Terminal> {
        self.provisioned.terminal(command)
    }

    pub fn read_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        self.provisioned.read_file(path)
    }

    /// Reads a path relative to the home the Instance's terminals and supervisor run with.
    pub fn read_home_file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        self.provisioned.read_home_file(path)
    }

    pub fn write_file(&mut self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.provisioned.write_file(path, contents)
    }

    /// Starts the Instance's supervisor, which alone is handed its link credential.
    pub fn supervise(&mut self, variables: &[(&str, &str)]) -> io::Result<Supervisor> {
        self.provisioned.supervise(variables)
    }

    pub fn destroy(mut self) -> io::Result<()> {
        self.provisioned.destroy()
    }
}

/// Outlives this handle as its Instance does: a control plane that restarts leaves it dialling
/// back in rather than taking it down.
pub struct Supervisor {
    name: String,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    supervising: Box<dyn Supervising>,
}

impl Supervisor {
    /// What `Driver::stop_named` stops it by once this handle is gone.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.stderr.take()
    }

    /// `None` while the supervisor is still running.
    pub fn status(&mut self) -> io::Result<Option<Exited>> {
        self.supervising.status()
    }

    pub fn stop(mut self) -> io::Result<()> {
        self.supervising.stop()
    }
}

/// What `exec` hands back while the command is still running, so its output can be read as it
/// arrives rather than only once it is over.
pub struct Streaming {
    child: Child,
}

impl Streaming {
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// Waits, collecting whichever of the two streams was not taken.
    pub fn finish(self) -> io::Result<Finished> {
        let finished = self.child.wait_with_output()?;

        Ok(Finished {
            exited: finished.status.into(),
            out: String::from_utf8_lossy(&finished.stdout).trim().to_owned(),
            err: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
        })
    }
}

#[derive(Debug)]
pub struct Finished {
    pub exited: Exited,
    pub out: String,
    pub err: String,
}

/// How something ended, in the only terms both drivers have: a container reports an exit code
/// and nothing else, and a process killed by a signal reports none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exited(Option<i32>);

impl Exited {
    pub const fn with(code: i32) -> Self {
        Self(Some(code))
    }

    pub const fn without_a_code() -> Self {
        Self(None)
    }

    pub fn success(&self) -> bool {
        self.0 == Some(0)
    }
}

impl From<ExitStatus> for Exited {
    fn from(status: ExitStatus) -> Self {
        Self(status.code())
    }
}

impl fmt::Display for Exited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(code) => write!(f, "with the code {code}"),
            None => f.write_str("without a code"),
        }
    }
}
