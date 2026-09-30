//! The same artifact `kestrel-env` ships, on a local-exec Instance, dialling out over the same
//! link the work role's would.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use kestrel::compute::{Driver, Exited, Instance, LocalExec, Supervisor as Supervising};
use kestrel::domain::SessionId;
use kestrel::link::Harness;

use super::diagnostics::Diagnostics;
use super::{OnTheLink, built, scripted_agent};

const PATIENCE: Duration = Duration::from_secs(30);

pub fn binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();

    BINARY.get_or_init(|| built::binary("kestrel-supervisor"))
}

fn driver() -> Driver {
    Driver::LocalExec(LocalExec::running(binary()))
}

pub struct Supervisor {
    instance: Instance,
    supervising: Supervising,
    diagnostics: Diagnostics,
    harness: String,
    model: String,
}

impl Supervisor {
    pub fn provision(link: &str, on: &OnTheLink) -> Self {
        Self::provision_playing(link, on, scripted_agent::Script::Speaks)
    }

    pub fn provision_playing(link: &str, on: &OnTheLink, script: scripted_agent::Script) -> Self {
        Self::provision_selecting(link, on, script, "")
    }

    pub fn provision_selecting(
        link: &str,
        on: &OnTheLink,
        script: scripted_agent::Script,
        model: &str,
    ) -> Self {
        let harness = scripted_agent::playing(script);

        Self::provision_running(link, on, &harness, model, None)
    }

    /// A supervisor told how long a Session's lease is held out for, so a control plane that is
    /// gone past it can be given up on. `None` is a supervisor with no bound to derive. The harness
    /// and model are what `harness` hands the Session's `start` to carry.
    pub fn provision_running(
        link: &str,
        on: &OnTheLink,
        harness: &str,
        model: &str,
        lease: Option<Duration>,
    ) -> Self {
        let mut instance = match driver()
            .resume(&on.instance)
            .expect("the instance should resume")
        {
            Some(instance) => instance,
            None => driver()
                .provision(on.provisioned_by)
                .expect("the instance should provision"),
        };
        assert_eq!(instance.name(), on.instance);
        let lease = lease.map(|lease| lease.as_secs().to_string());
        let mut variables = vec![
            ("KESTREL_LINK", link),
            ("KESTREL_INSTANCE", on.instance.as_str()),
            ("KESTREL_INSTANCE_CREDENTIAL", on.credential.as_str()),
        ];
        if let Some(lease) = lease.as_deref() {
            variables.push(("KESTREL_LEASE", lease));
        }
        let mut supervising = instance
            .supervise(&variables)
            .expect("the supervisor should spawn");

        let pipe = supervising
            .take_stderr()
            .expect("the supervisor's diagnostics should be piped");

        Self {
            instance,
            supervising,
            diagnostics: Diagnostics::pumped("the supervisor", pipe),
            harness: harness.to_owned(),
            model: model.to_owned(),
        }
    }

    /// What a Session's `start` carries for this supervisor to spawn.
    pub fn harness(&self) -> Harness {
        Harness {
            command: self.harness.clone(),
            auth: None,
            model: (!self.model.is_empty()).then(|| self.model.clone()),
        }
    }

    /// Once the supervisor has ended the Session's harness, for whatever reason; it stays on the
    /// link for the Instance.
    pub async fn lets_go_of(&mut self, session: SessionId) {
        self.wait_until_it_says(&format!("let the session {session} go"))
            .await;
    }

    pub async fn wait_until_it_says(&mut self, what: &str) {
        self.diagnostics.wait_until_it_says(what).await;
    }

    pub async fn exits(&mut self) -> Exited {
        let deadline = tokio::time::Instant::now() + PATIENCE;

        loop {
            if let Some(exited) = self
                .supervising
                .status()
                .expect("the supervisor should be waitable")
            {
                self.diagnostics.drain();
                return exited;
            }
            if tokio::time::Instant::now() > deadline {
                panic!(
                    "the supervisor is still running after {PATIENCE:?}. it said:\n{}",
                    self.everything_it_said()
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub fn said(&self, what: &str) -> bool {
        self.diagnostics.said(what)
    }

    pub fn everything_it_said(&self) -> String {
        self.diagnostics.everything_it_said()
    }

    pub fn destroy(self) {}

    pub fn instance(&self) -> &str {
        self.instance.name()
    }
}

/// The supervisor outlives any Session, so it goes with its Instance.
impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = driver().destroy_named(self.instance.name());
    }
}

impl Supervisor {
    pub async fn is_still_running(&mut self, after: Duration) -> bool {
        tokio::time::sleep(after).await;

        self.supervising
            .status()
            .expect("the supervisor should be waitable")
            .is_none()
    }
}
