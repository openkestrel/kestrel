//! These permanent live exceptions exercise OS process death, which an in-process work pass cannot reproduce.

use super::fixture::succeeded;
use crate::support;

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use support::client::{self, Finished, Invocation};
use support::scripted_agent::Script;
use tempfile::TempDir;

const PATIENCE: Duration = Duration::from_secs(30);

struct Kestrel {
    data_dir: TempDir,
}

impl Drop for Kestrel {
    fn drop(&mut self) {
        let data_dir = self.data_dir.path().to_path_buf();
        if !data_dir.join("kestrel.db").exists() {
            return;
        }
        let driver = kestrel::compute::Driver::LocalExec(kestrel::compute::LocalExec::running(
            support::supervisor::binary(),
        ));
        support::destroy_instances_on_drop(data_dir, Some(driver));
    }
}

struct Booted {
    child: Child,
    operator: String,
    stopped: bool,
}

impl Kestrel {
    fn new() -> Self {
        Self {
            data_dir: TempDir::new().expect("a temporary data directory"),
        }
    }

    fn booting(&self, listen: &str, script: Script, level: &str) -> Booted {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kestrel-control-plane"))
            .env("KESTREL_DATA_DIR", self.data_dir.path())
            .env("KESTREL_LISTEN", listen)
            .env("KESTREL_OPERATOR_LISTEN", "127.0.0.1:0")
            .env("KESTREL_COMPUTE", "local-exec")
            .env("KESTREL_SUPERVISOR", support::supervisor::binary())
            .env(
                "KESTREL_QUIET_PERIOD",
                support::QUIET_PERIOD.as_secs().to_string(),
            )
            .env(
                "KESTREL_HARNESS_COMMANDS",
                format!(
                    "{}={}",
                    support::HARNESS,
                    support::scripted_agent::playing(script)
                ),
            )
            .env("RUST_LOG", level)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the control plane should spawn");

        // Drained as it is said, so a chatty log cannot block the process on a full pipe.
        let stderr = child.stderr.take().expect("stderr should be piped");
        let said = Arc::new(Mutex::new(String::new()));
        let draining = Arc::clone(&said);
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut said = draining.lock().expect("the log should not be poisoned");
                said.push_str(&line);
                said.push('\n');
            }
        });
        let operator = operator_of(&said);

        Booted {
            child,
            operator,
            stopped: false,
        }
    }
}

impl Booted {
    fn client(&self, args: &[&str]) -> Finished {
        client::ran(&self.operator, args)
    }

    fn client_as(&self, args: &[&str], invocation: Invocation) -> Finished {
        client::ran_as(&self.operator, args, invocation)
    }

    fn run(&self, args: &[&str]) -> String {
        succeeded(args, &self.client(args))
    }

    fn run_as(&self, args: &[&str], invocation: Invocation) -> String {
        succeeded(args, &self.client_as(args, invocation))
    }

    fn records(&self, args: &[&str]) -> Vec<Value> {
        self.client(args).records()
    }

    fn record(&self, args: &[&str]) -> Value {
        self.client(args).json()
    }

    fn until(&self, args: &[&str], listed: impl Fn(&[Value]) -> bool, what: &str) -> Vec<Value> {
        let deadline = Instant::now() + PATIENCE;

        loop {
            let shown = self.records(args);
            if listed(&shown) {
                return shown;
            }
            assert!(
                Instant::now() < deadline,
                "`kestrel {}` never {what}. the last listing was:\n{shown:?}",
                args.join(" ")
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// `Child::kill` is a `SIGKILL`, so nothing the control plane holds in memory is given a
    /// chance to land.
    fn killed(mut self) {
        let _ = self.child.kill();
        self.child
            .wait()
            .expect("the control plane should be waitable");
        self.stopped = true;
    }

    /// Signalled rather than killed, so the work role sees a stopped Session's supervisor off the
    /// link before it goes.
    fn terminated(mut self) {
        #[allow(unsafe_code)]
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        self.child
            .wait()
            .expect("the control plane should be waitable");
        self.stopped = true;
    }
}

impl Drop for Booted {
    fn drop(&mut self) {
        if self.stopped || self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        #[cfg(unix)]
        #[allow(unsafe_code)]
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        #[cfg(not(unix))]
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn operator_of(said: &Mutex<String>) -> String {
    let deadline = Instant::now() + PATIENCE;

    loop {
        let started = said
            .lock()
            .expect("the log should not be poisoned")
            .lines()
            .find(|line| line.contains("role started") && line.contains("operator="))
            .map(str::to_owned);
        if let Some(line) = started {
            let address = line
                .split_whitespace()
                .find_map(|field| field.strip_prefix("operator="))
                .expect("the line should name the operator address");
            return format!("http://{address}");
        }
        assert!(
            Instant::now() < deadline,
            "the control plane never said where operators reach it:\n{}",
            said.lock().expect("the log should not be poisoned")
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A port nothing is listening on, so a control plane that is killed comes back on the
/// address the Environment it left behind already dialled.
fn a_free_port() -> String {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("a bound address")
        .port();

    format!("127.0.0.1:{port}")
}

fn sessions(kestrel: &Booted, workspace: &str) -> Vec<Value> {
    kestrel.records(&["session", "list", "--workspace", workspace, "--json"])
}

fn declared(kestrel: &Booted) {
    kestrel.run(&["organization", "declare", "acme"]);
    kestrel.run(&[
        "project",
        "declare",
        support::repository::NAME,
        "--repository",
        support::repository::url(),
        "--branch",
        support::repository::BRANCH,
    ]);
    kestrel.run(&[
        "agent",
        "declare",
        "builder",
        "--model",
        kestrel_scripted_agent::OTHER_MODEL,
    ]);
    kestrel.run_as(
        &["credential", "set", support::PROVIDER_KEY],
        Invocation::default().given(support::A_PROVIDER_KEY),
    );
}

fn opened(kestrel: &Booted) -> String {
    let opened = kestrel.record(&[
        "workspace",
        "open",
        "--project",
        support::repository::NAME,
        "--agent",
        "builder",
        "--json",
    ]);

    opened["workspace"]["name"]
        .as_str()
        .expect("the opened workspace's generated name")
        .to_owned()
}

/// Each entry as `seq kind …`, without the moment it was appended, which is different every
/// session.
fn transcribed(kestrel: &Booted, workspace: &str) -> Vec<String> {
    kestrel
        .records(&[
            "workspace",
            "transcript",
            workspace,
            "--no-summaries",
            "--json",
        ])
        .iter()
        .map(|recorded| {
            let entry = &recorded["entry"];
            let said = match entry["type"].as_str().expect("an entry kind") {
                "participant_joined" => format!("participant joined {}", entry["participant"]),
                "session_started" => {
                    format!("session started {} {}", entry["session"], entry["agent"])
                }
                "said" => format!("said {} {}", entry["participant"], entry["message"]),
                "brief" => format!("brief {}", entry["brief"]),
                "session_ended" => format!(
                    "session ended {} {}",
                    entry["session"], entry["exit"]["status"]
                ),
                "instance_released" => format!(
                    "instance released {} {}",
                    entry["participant"], entry["instance"]
                ),
                kind => kind.to_owned(),
            };
            format!("{} {}", recorded["seq"], said.replace('"', ""))
        })
        .collect()
}

#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_outlives_its_stopped_control_plane_and_goes_with_its_instance() {
    let kestrel = Kestrel::new();
    let booted = kestrel.booting("127.0.0.1:0", Script::Converses, "info");
    declared(&booted);
    let workspace = opened(&booted);
    booted.run(&[
        "workspace",
        "post",
        &workspace,
        "--as-participant",
        "operator",
        "go",
    ]);
    let listed = booted.until(
        &["session", "list", "--workspace", &workspace, "--json"],
        |listed| listed.iter().any(|session| session["state"] == "waiting"),
        "reach a waiting session",
    );
    let supervisor = listed[0]["supervisor"]
        .as_str()
        .expect("the waiting session has a supervisor")
        .to_owned();

    drop(booted);

    let supervisor = support::environment::Environment::named(&supervisor);
    assert!(
        supervisor
            .is_running(std::time::Duration::from_millis(500))
            .await,
        "the supervisor went with its control plane"
    );
    drop(kestrel);
    supervisor.is_gone().await;
}

#[cfg(unix)]
#[tokio::test]
async fn killing_a_control_plane_without_restarting_stops_its_supervisor() {
    let kestrel = Kestrel::new();
    let booted = kestrel.booting("127.0.0.1:0", Script::Converses, "info");
    declared(&booted);
    let workspace = opened(&booted);
    booted.run(&[
        "workspace",
        "post",
        &workspace,
        "--as-participant",
        "operator",
        "go",
    ]);
    let listed = booted.until(
        &["session", "list", "--workspace", &workspace, "--json"],
        |listed| listed.iter().any(|session| session["state"] == "waiting"),
        "reach a waiting session",
    );
    let supervisor = listed[0]["supervisor"].as_str().unwrap().to_owned();
    let instance = listed[0]["instance"].as_str().unwrap().to_owned();

    booted.killed();
    drop(kestrel);

    support::environment::Environment::named(&supervisor)
        .is_gone()
        .await;
    assert!(!support::environment::Environment::root_of(&instance).exists());
}

/// ADR-0002's definition of done for rung 0.1, out of process and against a real `SIGKILL`:
/// nothing the control plane held in memory lands, and the Environment it provisioned
/// outlives it.
#[tokio::test]
async fn a_control_plane_killed_mid_turn_comes_back_and_the_turn_is_answered() {
    let kestrel = Kestrel::new();
    let listen = a_free_port();
    let killed = kestrel.booting(&listen, Script::Lingers, "info");
    declared(&killed);
    let workspace = opened(&killed);
    let session = killed.record(&[
        "workspace",
        "post",
        &workspace,
        "--as-participant",
        "operator",
        "go",
        "--json",
    ])["session"]["id"]
        .as_str()
        .expect("the session the post reached")
        .parse::<kestrel::domain::SessionId>()
        .expect("the post must name the Session by its full identifier")
        .to_string();
    // The transcript says the Session started only once the supervisor holds the Start instruction,
    // which is the first moment a restart has anything to recover; an instance alone is not.
    killed.until(
        &["workspace", "transcript", &workspace, "--json"],
        |transcribed| {
            transcribed
                .iter()
                .any(|recorded| recorded["entry"]["type"] == "session_started")
        },
        "started its turn",
    );
    killed.killed();

    let restarted = kestrel.booting(&listen, Script::Lingers, "info");
    restarted.until(
        &["session", "list", "--workspace", &workspace, "--json"],
        |listed| {
            listed
                .iter()
                .any(|session| session["state"] == "waiting" || !session["exit"].is_null())
        },
        "answered its turn",
    );
    restarted.run(&["session", "stop", &session]);
    let listed = sessions(&restarted, &workspace);
    let supervisor = support::environment::Environment::named(
        listed[0]["supervisor"]
            .as_str()
            .expect("the Session supervisor"),
    );
    let transcript = transcribed(&restarted, &workspace);
    restarted.terminated();
    drop(kestrel);
    supervisor.is_gone().await;

    assert_eq!(
        listed[0]["exit"]["status"], "succeeded",
        "the session's turn was not answered after the restart: {listed:?}"
    );
    assert_eq!(
        transcript,
        vec![
            "1 participant joined builder".to_owned(),
            "2 participant joined operator".to_owned(),
            "3 brief go".to_owned(),
            format!("4 session started {session} builder"),
            "8 said builder half of one message, and the other half".to_owned(),
            "9 said builder a second message".to_owned(),
            format!("10 session ended {session} succeeded"),
        ]
    );
}
