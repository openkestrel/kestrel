//! The installed `kestrel` Client against a control plane booted as its own binary: nothing
//! here reaches the database except through the operator boundary.

use crate::support;

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::client::{self, Finished, Invocation};
use support::github_stub::{self, GithubStub};
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

/// A control plane running over a [`Kestrel`]'s data directory, and every line it has said.
struct Booted {
    child: Child,
    said: Arc<Mutex<String>>,
    operator: String,
    stopped: bool,
}

impl Kestrel {
    fn new() -> Self {
        Self {
            data_dir: TempDir::new().expect("a temporary data directory"),
        }
    }

    /// An ephemeral link port, so tests that boot one concurrently never race over kestrel's
    /// default.
    fn boot(&self) -> Booted {
        self.booting("127.0.0.1:0", Script::Speaks, "info")
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
            said,
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

    /// The refusal itself, so a test asserting one never passes on a command that succeeded.
    fn refused(&self, args: &[&str]) -> String {
        refusal(args, &self.client(args))
    }

    fn refused_as(&self, args: &[&str], invocation: Invocation) -> String {
        refusal(args, &self.client_as(args, invocation))
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

    fn said(&self) -> String {
        self.said
            .lock()
            .expect("the log should not be poisoned")
            .clone()
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

fn succeeded(args: &[&str], finished: &Finished) -> String {
    assert!(
        finished.status.success(),
        "`kestrel {}` failed with {}:\n{}",
        args.join(" "),
        finished.status,
        finished.err
    );
    finished.out.join("\n")
}

fn refusal(args: &[&str], finished: &Finished) -> String {
    assert!(
        !finished.status.success(),
        "`kestrel {}` was expected to be refused, and succeeded",
        args.join(" ")
    );
    finished.err.clone()
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

/// Answering a turn never ends a Session, so one waiting is stopped the way a person would.
fn dispatched(kestrel: &Booted, workspace: &str) -> Vec<Value> {
    let deadline = Instant::now() + PATIENCE;

    loop {
        let listed = sessions(kestrel, workspace);
        if listed
            .iter()
            .all(|session| !session["exit"]["status"].is_null())
        {
            return listed;
        }
        for waiting in listed
            .iter()
            .filter(|session| session["state"] == "waiting")
        {
            let session = waiting["id"].as_str().expect("a session's identifier");
            kestrel.run(&["session", "stop", session]);
        }
        assert!(
            Instant::now() < deadline,
            "no session in the workspace {workspace} ended. the last listing was:\n{listed:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
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

#[test]
fn a_session_show_says_the_title_its_options_and_its_commands() {
    let kestrel = Kestrel::new();
    let booted = kestrel.booting("127.0.0.1:0", Script::Announces, "info");
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
        |listed| listed.iter().any(|session| !session["title"].is_null()),
        "show the harness's title",
    );
    let session = listed
        .iter()
        .find(|session| !session["title"].is_null())
        .and_then(|session| session["id"].as_str())
        .expect("the session's identifier")
        .to_owned();

    let shown = booted.run(&["session", "show", &session]);

    assert!(
        shown.contains(kestrel_scripted_agent::TITLE),
        "the title is not shown:\n{shown}"
    );
    assert!(
        shown.contains("model: scripted-max"),
        "the model option's current value is not shown as `category: current`:\n{shown}"
    );
    assert!(
        shown.contains("mode: plan"),
        "the mode option's current value is not shown:\n{shown}"
    );
    assert!(
        shown.contains("compact"),
        "the commands are not shown:\n{shown}"
    );

    booted.terminated();
}

#[test]
fn a_session_option_set_changes_an_option_and_warns_about_the_cache() {
    let kestrel = Kestrel::new();
    let booted = kestrel.booting("127.0.0.1:0", Script::Speaks, "info");
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

    let waiting = booted.until(
        &["session", "list", "--workspace", &workspace, "--json"],
        |listed| listed.iter().any(|session| session["state"] == "waiting"),
        "reach a waiting session",
    );
    let session = waiting
        .iter()
        .find(|session| session["state"] == "waiting")
        .and_then(|session| session["id"].as_str())
        .expect("the waiting session's identifier")
        .to_owned();
    // The bookkeeping report is debounced, so the option is waited for before it is changed.
    booted.until(
        &["session", "show", &session, "--json"],
        |shown| {
            shown[0]["options"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|option| option["category"] == "model")
        },
        "report its options",
    );

    let changed = booted.client(&[
        "session",
        "option",
        "set",
        &session,
        "model",
        kestrel_scripted_agent::OTHER_MODEL,
        "--as-participant",
        "operator",
    ]);
    assert!(
        changed.status.success(),
        "`session option set` failed:\n{}",
        changed.err
    );
    assert!(
        changed.err.contains("1200"),
        "the cache warning did not name the context it re-reads:\n{}",
        changed.err
    );

    let settled = booted.until(
        &["session", "show", &session, "--json"],
        |shown| {
            let shown = &shown[0];
            shown["changing_options"]
                .as_array()
                .is_some_and(Vec::is_empty)
                && shown["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|option| {
                        option["category"] == "model"
                            && option["current"] == kestrel_scripted_agent::OTHER_MODEL
                    })
        },
        "apply the changed model",
    );
    assert_eq!(
        settled[0]["changing_options"].as_array().map(Vec::len),
        Some(0)
    );

    booted.terminated();
}

#[test]
fn workspace_open_declares_the_mode_its_first_session_runs_in() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);
    let workspace = booted.record(&[
        "workspace",
        "open",
        "--project",
        support::repository::NAME,
        "--agent",
        "builder",
        "--mode",
        kestrel_scripted_agent::SWITCHED_MODE,
        "--brief",
        "go",
        "--as-participant",
        "operator",
        "--json",
    ])["workspace"]["name"]
        .as_str()
        .expect("the opened workspace's name")
        .to_owned();

    let listed = booted.until(
        &["session", "list", "--workspace", &workspace, "--json"],
        |listed| listed.iter().any(|session| session["state"] == "waiting"),
        "answer its first turn",
    );
    let session = listed[0]["id"].as_str().expect("the session's identifier");
    let shown: Value = serde_json::from_str(&booted.run(&["session", "show", session, "--json"]))
        .expect("the session's fields as JSON");

    assert_eq!(
        shown["mode"],
        kestrel_scripted_agent::SWITCHED_MODE,
        "the mode the open declared was not set on the harness: {shown}"
    );

    booted.terminated();
}

#[test]
fn a_role_boots_on_an_empty_data_directory_and_makes_its_database() {
    let kestrel = Kestrel::new();

    let booted = kestrel.boot();

    assert!(kestrel.data_dir.path().join("kestrel.db").exists());
    assert_eq!(booted.run(&["organization", "list"]), "");
}

#[test]
fn a_disabled_trigger_shows_its_reason_and_budget() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);
    booted.run(&[
        "trigger",
        "declare",
        "ready",
        "--filter",
        r#"{"exact": {"type": "com.github.issues.labeled"}}"#,
        "--brief",
        "Work on {{ event.data.issue.title }}",
        "--project",
        "kestrel",
        "--agent",
        "builder",
    ]);
    booted.run(&["trigger", "disable", "ready"]);

    let trigger = booted.record(&["trigger", "show", "ready", "--json"]);

    assert_eq!(trigger["state"], "disabled:operator");
    assert_eq!(trigger["disabled_because"], "disabled by an operator");
    assert_eq!(trigger["firing_budget"]["limit"], 10);
}

#[test]
fn an_agents_model_changes_without_declaring_the_agent_again() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    booted.run(&["organization", "declare", "acme"]);
    booted.run(&[
        "agent",
        "declare",
        "builder",
        "--harness",
        "codex",
        "--model",
        "claude-opus-5",
    ]);

    assert_eq!(
        booted.run(&["agent", "model", "builder", "--model", "claude-sonnet-5"]),
        "claude-sonnet-5"
    );
    let agents = booted.records(&["agent", "list", "--json"]);
    assert_eq!(agents.len(), 1, "{agents:?}");
    assert_eq!(agents[0]["name"], "builder");
    assert_eq!(agents[0]["harness"], "codex");
    assert_eq!(agents[0]["model"], "claude-sonnet-5");
    assert_eq!(
        booted.record(&["agent", "model", "builder", "--json"])["model"],
        Value::Null
    );
    assert!(
        booted
            .refused(&["agent", "model", "reviewer", "--model", "claude-sonnet-5"])
            .contains("no agent named reviewer"),
    );
}

#[test]
fn an_instance_is_shown_on_its_workspace_and_released_on_the_record() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
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
    dispatched(&booted, &workspace);

    let shown = booted.record(&["workspace", "show", &workspace, "--json"]);
    let instance = shown["instance"]
        .as_str()
        .expect("the workspace keeps its instance")
        .to_owned();
    assert_eq!(
        shown["held"],
        Value::Null,
        "a checkout the remote can restore was held"
    );
    assert_eq!(booted.run(&["instance", "list"]), "");

    assert_eq!(booted.run(&["instance", "release", &workspace]), instance);

    assert_eq!(
        booted.record(&["workspace", "show", &workspace, "--json"])["instance"],
        Value::Null,
        "a released instance is still the workspace's"
    );
    assert_eq!(
        transcribed(&booted, &workspace).last(),
        Some(&format!("11 instance released operator {instance}")),
        "the release is not on the record"
    );
    assert!(
        booted
            .refused(&["instance", "release", &workspace])
            .contains("no instance"),
    );
}

#[test]
fn a_workspace_post_without_a_participant_is_a_usage_error() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);
    let workspace = opened(&booted);

    let refused = booted.refused(&["workspace", "post", &workspace, "go"]);

    assert!(
        refused.contains("--as-participant"),
        "a post without a participant should be refused by usage: {refused}"
    );
}

#[test]
fn a_held_message_is_printed_listed_edited_and_withdrawn() {
    let kestrel = Kestrel::new();
    let booted = kestrel.booting("127.0.0.1:0", Script::Dawdles, "info");
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
    booted.until(
        &["session", "list", "--workspace", &workspace, "--json"],
        |listed| listed.iter().any(|session| session["state"] == "working"),
        "reach a working session",
    );

    let held = booted.run(&[
        "workspace",
        "post",
        &workspace,
        "--as-participant",
        "alice",
        "one more change",
    ]);
    let id: i64 = held.parse().expect("the post prints the held id");
    let listed = booted.record(&["workspace", "show", &workspace, "--json"]);
    assert_eq!(listed["held_messages"][0]["id"], id);
    assert_eq!(listed["held_messages"][0]["participant"], "alice");
    assert_eq!(listed["held_messages"][0]["message"], "one more change");

    let edited = booted.record(&[
        "workspace",
        "message",
        "edit",
        &workspace,
        &held,
        "--as-participant",
        "alice",
        "the edited change",
        "--json",
    ]);
    assert_eq!(edited["id"], id);
    assert_eq!(edited["message"], "the edited change");
    assert!(!edited["edited_at"].is_null(), "{edited}");

    let refused = booted.refused(&[
        "workspace",
        "message",
        "edit",
        &workspace,
        &held,
        "--as-participant",
        "bob",
        "mine now",
    ]);
    assert!(
        refused.contains("only its author may change it"),
        "{refused}"
    );

    booted.run(&[
        "workspace",
        "message",
        "withdraw",
        &workspace,
        &held,
        "--as-participant",
        "alice",
    ]);
    assert_eq!(
        booted.record(&["workspace", "show", &workspace, "--json"])["held_messages"],
        serde_json::json!([])
    );
    let refused = booted.refused(&[
        "workspace",
        "message",
        "withdraw",
        &workspace,
        &held,
        "--as-participant",
        "alice",
    ]);
    assert!(refused.contains("already withdrawn"), "{refused}");
}

#[test]
fn a_session_interrupt_names_who_asked_and_a_waiting_one_is_refused() {
    let kestrel = Kestrel::new();
    let booted = kestrel.booting("127.0.0.1:0", Script::WorksUntilCancelled, "info");
    declared(&booted);
    // Opened with a Brief, so the session's first turn is its instruction and nothing races it.
    let workspace = booted.record(&[
        "workspace",
        "open",
        "--project",
        support::repository::NAME,
        "--agent",
        "builder",
        "--brief",
        "go",
        "--json",
    ])["workspace"]["name"]
        .as_str()
        .expect("the opened workspace's name")
        .to_owned();
    let listed = booted.until(
        &["session", "list", "--workspace", &workspace, "--json"],
        |listed| listed.iter().any(|session| session["state"] == "working"),
        "reach a working session",
    );
    let session = listed[0]["id"]
        .as_str()
        .expect("the working session's id")
        .to_owned();

    let interrupted = booted.record(&[
        "session",
        "interrupt",
        &session,
        "--as-participant",
        "alice",
        "--json",
    ]);
    assert_eq!(interrupted["id"], session);
    assert_eq!(interrupted["state"], "working");
    assert_eq!(interrupted["interrupting"]["participant"], "alice");

    let waiting = booted.until(
        &["session", "show", &session, "--json"],
        |shown| shown.iter().any(|session| session["state"] == "waiting"),
        "settle waiting",
    );
    assert_eq!(waiting[0]["state"], "waiting");
    assert!(
        booted
            .refused(&[
                "session",
                "interrupt",
                &session,
                "--as-participant",
                "alice"
            ])
            .contains("waiting"),
    );

    let entries = booted.records(&[
        "workspace",
        "transcript",
        &workspace,
        "--kinds",
        "shared_state,narration,detail",
        "--json",
    ]);
    assert!(
        entries.iter().any(|recorded| {
            recorded["entry"]["type"] == "turn_interrupted"
                && recorded["entry"]["participant"] == "alice"
        }),
        "{entries:?}"
    );
}

#[test]
fn a_session_ends_succeeded_while_waiting_and_is_not_stopped_twice() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
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

    let listed = dispatched(&booted, &workspace);
    let session = listed[0]["id"]
        .as_str()
        .expect("the session's id")
        .to_owned();

    assert_eq!(listed[0]["exit"]["status"], "succeeded");
    assert!(
        listed[0]["instance"]
            .as_str()
            .is_some_and(|instance| instance.starts_with("local-exec/")),
        "the session does not list the instance it executed on: {listed:?}"
    );
    assert_eq!(
        listed[0]["worked_model"],
        kestrel_scripted_agent::OTHER_MODEL
    );
    assert!(
        booted
            .refused(&["session", "stop", &session])
            .contains("has already ended"),
    );
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
#[test]
fn a_control_plane_killed_mid_turn_comes_back_and_the_turn_is_answered() {
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
        .to_owned();
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
    let transcript = transcribed(&restarted, &workspace);
    // The supervisor belongs to the control plane that was killed, so this one cannot wait it off
    // the link on the way down; it leaves within a poll of the stop reaching it.
    std::thread::sleep(Duration::from_secs(1));
    restarted.terminated();

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

/// A Client hands the control plane every secret it holds over the operator boundary, and
/// the control plane says none of them back while it takes them.
#[test]
fn secrets_set_through_the_client_appear_in_no_log_line() {
    let kestrel = Kestrel::new();
    let stub = GithubStub::start();
    let booted = kestrel.booting("127.0.0.1:0", Script::Speaks, "trace");
    let provider_key = "sk-kestrel-should-never-say-this-either";
    let signing_secret = "whsec-kestrel-should-never-say-this";

    let mut printed = booted.run(&["organization", "declare", "acme"]);
    printed += &booted.run_as(
        &["credential", "set", "ANTHROPIC_API_KEY"],
        Invocation::default().given(provider_key),
    );
    let registering = [
        "integration",
        "register",
        "github",
        "hub",
        "--repository",
        "jtmthf/kestrel",
        "--app-id",
        "1",
        "--installation",
        "2",
        "--private-key",
        support::PRIVATE_KEY,
        "--api",
        &stub.base_url(),
        "--webhook-secret",
        signing_secret,
    ];
    printed += &booted.run(&registering);
    printed += &booted.refused(&registering);
    printed += &booted.run(&["credential", "list"]);
    printed += &booted.run(&["integration", "list"]);
    let said = booted.said();
    booted.killed();

    assert!(
        said.contains("INSERT INTO provider_credential")
            && said.contains("INSERT INTO integration"),
        "the control plane logged nothing of what it was asked to hold:\n{said}"
    );
    for (name, secret) in [
        ("the provider key", provider_key),
        ("the private key", support::PRIVATE_KEY),
        ("the signing secret", signing_secret),
    ] {
        assert!(!printed.contains(secret), "the client printed {name}");
        assert!(!said.contains(secret), "a log line spelled {name} out");
    }
}

fn watching(kestrel: &Booted, stub: &GithubStub, interval: &str) {
    kestrel.run(&["organization", "declare", "acme"]);
    kestrel.run(&[
        "integration",
        "register",
        "github",
        "hub",
        "--repository",
        "jtmthf/kestrel",
        "--app-id",
        "1",
        "--installation",
        "2",
        "--private-key",
        support::PRIVATE_KEY,
        "--api",
        &stub.base_url(),
        "--interval",
        interval,
    ]);
}

/// The one command that has a credential in it, and the whole of what kestrel says while it
/// uses it: neither the listing an operator reads nor the log they debug from has the token.
#[test]
fn the_client_lists_what_a_poll_recorded_and_the_credential_appears_in_neither_it_nor_a_log() {
    let kestrel = Kestrel::new();
    let stub = GithubStub::start();
    let delivery = stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    let booted = kestrel.booting("127.0.0.1:0", Script::Speaks, "trace");
    watching(&booted, &stub, "1ms");

    let listed = booted.until(
        &["event", "list", "--json"],
        |listed| !listed.is_empty(),
        "listed an event polled from github",
    );
    let said = booted.said();
    let record = listed[0]["record"].as_str().expect("an event record");
    let shown = booted.record(&["event", "show", record, "--json"]);
    booted.killed();

    assert_eq!(shown["record"], record);
    assert_eq!(shown["event"]["id"], delivery);
    assert_eq!(shown["event"]["specversion"], "1.0");
    assert_eq!(shown["event"]["subject"], "#43");
    let listed = serde_json::to_string(&listed).expect("the listing serializes");
    assert!(
        listed.contains("ready-for-agent"),
        "an event listing that does not say what happened:\n{listed}"
    );
    assert!(
        !listed.contains(support::PRIVATE_KEY),
        "the listing spelled the credential out"
    );
    assert!(
        !said.contains(support::PRIVATE_KEY),
        "a log line spelled the credential out"
    );
    assert!(
        said.contains("a poll recorded events"),
        "the control plane never said it polled:\n{said}"
    );
}

#[test]
fn a_dispatch_starts_a_triggers_work_on_the_issue_it_names() {
    let kestrel = Kestrel::new();
    let stub = GithubStub::start();
    stub.script_answer("GET", "/issues/60", github_stub::issue(60, &[]));
    let booted = kestrel.boot();
    declared(&booted);
    booted.run(&[
        "integration",
        "register",
        "github",
        "hub",
        "--repository",
        "jtmthf/kestrel",
        "--app-id",
        "1",
        "--installation",
        "2",
        "--private-key",
        support::PRIVATE_KEY,
        "--api",
        &stub.base_url(),
        "--carries",
        "outbound",
    ]);
    booted.run(&[
        "trigger",
        "declare",
        "delegated",
        "--filter",
        r#"{"exact": {"type": "com.github.issue_comment.created"}}"#,
        "--brief",
        "{{ instruction }} {{ event.data.issue.number }}",
        "--branch",
        "kestrel/issue-{{ event.data.issue.number }}",
        "--project",
        "kestrel",
        "--agent",
        "builder",
    ]);

    let tested = booted.record(&[
        "trigger",
        "test",
        "delegated",
        "--integration",
        "hub",
        "--issue",
        "60",
        "--instruction",
        "/tdd the parser",
        "--json",
    ]);
    assert!(booted.records(&["event", "list", "--json"]).is_empty());

    let fired = booted.record(&[
        "trigger",
        "dispatch",
        "delegated",
        "--integration",
        "hub",
        "--issue",
        "60",
        "--instruction",
        "/tdd the parser",
        "--json",
    ]);

    assert_eq!(tested["matches"], true, "{tested}");
    assert_eq!(tested["brief"], "/tdd the parser 60", "{tested}");
    assert_eq!(tested["branch"], "kestrel/issue-60", "{tested}");
    assert_eq!(tested["agent"], "builder", "{tested}");
    assert_eq!(fired["outcome"], "opened");
    let workspace = fired["workspace"]
        .as_str()
        .expect("the workspace it opened");
    let shown = booted.record(&["workspace", "show", workspace, "--json"]);
    assert_eq!(shown["checkout"]["branch"], "kestrel/issue-60");
    for misused in [
        &["trigger", "test", "delegated", "--issue", "60"][..],
        &[
            "trigger",
            "test",
            "delegated",
            "--integration",
            "hub",
            "--issue",
            "60",
            "--event",
            "x",
        ],
    ] {
        assert_eq!(
            booted.client(misused).status.code(),
            Some(2),
            "`kestrel {}` is a usage error",
            misused.join(" ")
        );
    }
    assert!(
        booted
            .refused(&[
                "trigger",
                "dispatch",
                "delegated",
                "--integration",
                "nowhere",
                "--issue",
                "60",
            ])
            .contains("no integration named nowhere"),
    );
}

const APPLIED: &str = r#"
triggers:
  ready:
    filter: {exact: {type: com.github.issues.labeled}}
    brief: Work on {{ event.data.issue.title }}
    project: kestrel
    agent: builder
  triage:
    filter:
      all:
        - exact: {type: com.github.issues.opened}
        - exact: {data.issue.author_association: MEMBER}
    brief: Triage {{ event.data.issue.title }}
    project: kestrel
    agent: builder
"#;

fn applying(kestrel: &Booted, declarations: &str, flags: &[&str]) -> Finished {
    let mut args = vec!["trigger", "apply", "-f", "triggers.yaml"];
    args.extend_from_slice(flags);
    kestrel.client_as(
        &args,
        Invocation::default().file("triggers.yaml", declarations),
    )
}

/// What it printed, and what it warned of.
fn applied(kestrel: &Booted, declarations: &str, flags: &[&str]) -> (String, String) {
    let finished = applying(kestrel, declarations, flags);
    let diff = succeeded(&["trigger", "apply"], &finished);
    (diff, finished.err)
}

fn trigger_names(kestrel: &Booted) -> Vec<String> {
    kestrel
        .records(&["trigger", "list", "--json"])
        .iter()
        .map(|trigger| trigger["name"].as_str().expect("a name").to_owned())
        .collect()
}

#[test]
fn apply_prints_the_diff_it_makes_and_nothing_once_it_is_made() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);

    let (diff, _) = applied(&booted, APPLIED, &[]);

    assert!(diff.contains("+ ready\n"), "{diff}");
    assert!(diff.contains("+ triage\n"), "{diff}");
    assert!(
        diff.contains("    brief\n      + Work on {{ event.data.issue.title }}\n"),
        "{diff}"
    );
    assert_eq!(trigger_names(&booted), ["ready", "triage"]);
    assert_eq!(
        booted.record(&["trigger", "show", "ready", "--json"])["applied"],
        true
    );

    assert_eq!(applied(&booted, APPLIED, &[]).0, "no changes");
}

#[test]
fn reapplying_changes_and_removes_only_what_a_file_applied() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);
    booted.run(&[
        "trigger",
        "declare",
        "one-off",
        "--filter",
        r#"{"exact": {"type": "com.example.build.failed"}}"#,
        "--brief",
        "Fix the build",
        "--project",
        "kestrel",
        "--agent",
        "builder",
    ]);
    applied(&booted, APPLIED, &[]);

    let changed = r#"
triggers:
  ready:
    filter: {exact: {type: com.github.issues.labeled}}
    brief: Work {{ event.data.issue.html_url }}
    project: kestrel
    agent: builder
"#;
    let (diff, _) = applied(&booted, changed, &[]);

    assert_eq!(
        diff,
        "~ ready\n    brief\n      - Work on {{ event.data.issue.title }}\n      + Work {{ event.data.issue.html_url }}\n- triage"
    );
    assert_eq!(trigger_names(&booted), ["one-off", "ready"]);
}

#[test]
fn a_one_off_a_file_declares_becomes_the_files() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);
    booted.run(&[
        "trigger",
        "declare",
        "ready",
        "--filter",
        r#"{"exact": {"type": "com.github.issues.labeled"}}"#,
        "--brief",
        "Work on {{ event.data.issue.title }}",
        "--project",
        "kestrel",
        "--agent",
        "builder",
    ]);

    let (diff, _) = applied(&booted, APPLIED, &[]);

    assert!(
        diff.contains("~ ready\n    declared by\n      - flags\n      + a file\n"),
        "{diff}"
    );
    applied(&booted, "triggers: {}", &[]);
    assert!(trigger_names(&booted).is_empty());
}

#[test]
fn a_dry_run_prints_the_diff_and_changes_nothing() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);

    let (diff, warned) = applied(&booted, APPLIED, &["--dry-run"]);

    assert!(diff.contains("+ ready\n"), "{diff}");
    assert!(trigger_names(&booted).is_empty());
    assert!(warned.contains("the trigger ready fires"), "{warned}");
    assert!(warned.contains("0.4"), "{warned}");
    assert!(!warned.contains("the trigger triage"), "{warned}");
}

#[test]
fn an_apply_that_cannot_be_made_whole_changes_nothing() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);
    let naming_a_stranger = format!(
        "{APPLIED}  stranger:\n    filter: {{exact: {{type: x}}}}\n    brief: x\n    project: kestrel\n    agent: nobody\n"
    );

    let refusal = refusal(
        &["trigger", "apply"],
        &applying(&booted, &naming_a_stranger, &[]),
    );

    assert!(refusal.contains("nobody"), "{refusal}");
    assert!(trigger_names(&booted).is_empty());
}

#[test]
fn a_declaration_file_that_is_not_one_is_refused_saying_where() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);

    for (declarations, because) in [
        ("", "triggers"),
        ("triggers:\n  ready:\n    brief: x\n", "filter"),
        (
            "triggers:\n  ready:\n    filter: {sql: x}\n    brief: x\n    project: kestrel\n    agent: builder\n",
            "the trigger ready",
        ),
        (
            "triggers:\n  ready:\n    filter: {exact: {type: x}}\n    brief: x\n    correlation: x\n    project: kestrel\n    agent: builder\n",
            "misses",
        ),
        (
            "triggers:\n  ready:\n    filter: {exact: {type: x}}\n    brief: x\n    agnet: builder\n    project: kestrel\n",
            "agnet",
        ),
    ] {
        let said = refusal(&["trigger", "apply"], &applying(&booted, declarations, &[]));
        assert!(
            said.contains(because),
            "{declarations:?} was refused unhelpfully: {said}"
        );
    }
}

#[test]
fn apply_reads_a_declaration_file_from_standard_input() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);

    booted.run_as(
        &["trigger", "apply", "-f", "-"],
        Invocation::default().given(APPLIED),
    );

    assert_eq!(trigger_names(&booted), ["ready", "triage"]);
}

#[test]
fn a_trigger_is_tested_as_a_file_declares_it_rather_than_as_it_was_applied() {
    let kestrel = Kestrel::new();
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    let booted = kestrel.boot();
    declared(&booted);
    watching(&booted, &stub, "1ms");
    booted.run(&[
        "trigger",
        "declare",
        "ready",
        "--filter",
        r#"{"exact": {"type": "com.github.issues.opened"}}"#,
        "--brief",
        "as applied",
        "--project",
        "kestrel",
        "--agent",
        "builder",
    ]);
    let event = booted.until(
        &["event", "list", "--json"],
        |listed| !listed.is_empty(),
        "listed an event polled from github",
    )[0]["record"]
        .as_str()
        .expect("an event record")
        .to_owned();
    let declaring = "triggers:\n  ready:\n    filter: {exact: {type: com.github.issues.labeled}}\n    brief: '{{ instruction }} {{ event.subject }}'\n    project: kestrel\n    agent: builder\n";

    let tested = booted
        .client_as(
            &[
                "trigger",
                "test",
                "ready",
                "--event",
                &event,
                "-f",
                "triggers.yaml",
                "--instruction",
                "@instruction.md",
                "--json",
            ],
            Invocation::default()
                .file("triggers.yaml", declaring)
                .file("instruction.md", "/triage"),
        )
        .json();

    assert_eq!(tested["matches"], true, "{tested}");
    assert_eq!(tested["brief"], "/triage #43", "{tested}");
    assert_eq!(
        booted.record(&["trigger", "test", "ready", "--event", &event, "--json"])["matches"],
        false
    );
    assert!(
        booted
            .refused_as(
                &[
                    "trigger",
                    "test",
                    "elsewhere",
                    "--event",
                    &event,
                    "-f",
                    "triggers.yaml"
                ],
                Invocation::default().file("triggers.yaml", declaring),
            )
            .contains("declares no trigger elsewhere"),
    );
}

#[test]
fn a_brief_and_a_filter_are_read_from_a_file_or_standard_input() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);

    let finished = booted.client_as(
        &[
            "trigger",
            "declare",
            "ready",
            "--filter",
            "-",
            "--brief",
            "@brief.md",
            "--project",
            "kestrel",
            "--agent",
            "builder",
        ],
        Invocation::default()
            .given(r#"{"exact": {"type": "com.github.issues.labeled"}}"#)
            .file(
                "brief.md",
                "Work on {{ event.data.issue.title }}\n\nand say so.\n",
            ),
    );
    succeeded(&["trigger", "declare"], &finished);

    let shown = booted.record(&["trigger", "show", "ready", "--json"]);
    assert_eq!(
        shown["filter"],
        serde_json::json!({ "exact": { "type": "com.github.issues.labeled" } })
    );
    assert!(
        shown["brief"]
            .as_str()
            .is_some_and(|brief| brief.ends_with("and say so.\n")),
        "{shown}"
    );
    assert_eq!(shown["applied"], false);
    assert!(
        finished.err.contains("the trigger ready fires"),
        "a one-off admitting outsiders was declared without a warning"
    );
}

#[test]
fn only_one_of_a_filter_and_a_brief_is_read_from_standard_input() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    declared(&booted);

    let refusal = booted.refused(&[
        "trigger",
        "declare",
        "ready",
        "--filter",
        "-",
        "--brief",
        "-",
        "--project",
        "kestrel",
        "--agent",
        "builder",
    ]);

    assert!(refusal.contains("standard input"), "{refusal}");
}

#[test]
fn transcript_kinds_select_the_entries_the_client_streams() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
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
    dispatched(&booted, &workspace);
    let shared = booted.records(&[
        "workspace",
        "transcript",
        &workspace,
        "--no-summaries",
        "--json",
    ]);
    assert!(shared.iter().all(|record| record["kind"] == "shared_state"));
    let narration = booted.records(&[
        "workspace",
        "transcript",
        &workspace,
        "--kinds",
        "narration",
        "--no-summaries",
        "--json",
    ]);
    assert_eq!(narration.len(), 2);
    assert_eq!(narration[0]["entry"]["type"], "plan");
    assert_eq!(narration[1]["entry"]["type"], "thought");
    assert!(
        narration
            .iter()
            .all(|record| record["kind"] == "narration" && record["session_id"].is_string())
    );
    let all = booted.records(&[
        "workspace",
        "transcript",
        &workspace,
        "--kinds",
        "narration,shared_state",
        "--no-summaries",
        "--json",
    ]);
    assert_eq!(all.len(), shared.len() + narration.len());
    assert!(
        all.windows(2)
            .all(|pair| pair[0]["seq"].as_i64() < pair[1]["seq"].as_i64())
    );
}

#[test]
fn empty_cli_lists_guide_a_terminal_and_leave_pipes_empty() {
    let kestrel = Kestrel::new();
    let booted = kestrel.boot();
    let args = ["organization", "list"];
    let shown = client::ran_on_a_terminal(&booted.operator, &args, 120, "");
    assert!(shown.status.success(), "{}", shown.said);
    assert!(shown.said.contains("No Organizations."), "{}", shown.said);
    assert!(
        shown.said.contains("kestrel organization declare --help"),
        "{}",
        shown.said
    );
    assert!(booted.client(&args).out.is_empty());
    let json = client::ran_on_a_terminal(
        &booted.operator,
        &["organization", "list", "--json"],
        120,
        "",
    );
    assert!(json.status.success(), "{}", json.said);
    assert!(!json.said.contains("No Organizations"), "{}", json.said);
    booted.run(&["organization", "declare", "acme"]);
    for (command, absent, action) in [
        ("project", "Projects", "project declare --help"),
        ("agent", "Agents", "agent declare --help"),
        (
            "credential",
            "Provider Credentials",
            "credential set --help",
        ),
        ("profile", "Subscription Profiles", "profile declare --help"),
        (
            "integration",
            "Integrations",
            "integration register github --help",
        ),
        ("event", "Events", "integration list"),
        ("trigger", "Triggers", "trigger declare --help"),
        ("workspace", "Workspaces", "workspace open --help"),
        ("instance", "Instances", "workspace list"),
    ] {
        let args = ["--organization", "acme", command, "list"];
        let shown = client::ran_on_a_terminal(&booted.operator, &args, 120, "");
        assert!(shown.status.success(), "{}", shown.said);
        assert!(
            shown
                .said
                .contains(&format!("No {absent} in Organization acme.")),
            "{}",
            shown.said
        );
        assert!(
            shown
                .said
                .contains(&format!("kestrel --organization acme {action}")),
            "{}",
            shown.said
        );
        let piped = booted.client(&args);
        assert!(piped.status.success(), "{}", piped.err);
        assert!(piped.out.is_empty(), "{:?}", piped.out);
        assert!(piped.err.is_empty(), "{}", piped.err);
        assert!(!shown.said.contains("Compose"));
        assert!(!shown.said.contains("Participant"));
    }
}

#[test]
fn work_inspection_help_explains_comparisons_scopes_and_paths() {
    for (command, description) in [
        ("work", "reported snapshot"),
        ("changes", "Before the first push"),
        ("commits", "no remote-tracking branch reaches"),
        ("stashes", "without diffing them"),
    ] {
        let finished = client::ran("http://127.0.0.1:1", &["workspace", command, "--help"]);
        assert!(finished.status.success(), "{}", finished.err);
        let shown = finished.out.join("\n");
        for required in [
            description,
            "checkout base",
            "work branch",
            "<repo>/<path>",
            "kestrel workspace",
            "latest",
        ] {
            assert!(
                shown.contains(required),
                "{command} help lacks {required}: {shown}"
            );
        }
        assert!(!shown.contains('\u{1b}'), "{shown}");
    }
}

#[test]
fn empty_cli_inspection_lists_leave_pipes_empty_and_json_unmodified() {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("a stub operator");
    let operator = format!("http://{}", server.server_addr());
    let serving = std::thread::spawn(move || {
        for index in 0..19 {
            let request = server.recv().expect("an inspection request");
            let answer = if request.url().contains("/work") && index >= 17 {
                json!({"state":"reported", "reported_at":"2026-10-07T12:00:00Z", "repositories":[{"repository":"repo", "git":"unreadable", "because":"checkout unavailable"}], "last_report":{"report":"none"},"earlier_reports":[]})
            } else if index >= 15 {
                json!({"repositories":[{"repository":"repo", "text":"abcd1234 a local commit\n"}]})
            } else if request.url().contains("/sessions") {
                json!([])
            } else if request.url().contains("/files") {
                json!({"path":"repo/empty", "entries":[], "total":0, "truncated":false})
            } else if request.url().contains("/changes") {
                json!({"repositories":[{"repository":"repo", "diff":"", "stats":[], "truncated":false}]})
            } else {
                json!({"repositories":[{"repository":"repo", "text":""}]})
            };
            request
                .respond(tiny_http::Response::from_string(answer.to_string()))
                .expect("an operator answer");
        }
    });
    for (args, absent) in [
        (
            vec!["workspace", "changes", "latest"],
            "changes in this comparison",
        ),
        (vec!["workspace", "commits", "latest"], "commits"),
        (vec!["workspace", "stashes", "latest"], "stashes"),
        (
            vec!["workspace", "files", "latest", "repo/empty"],
            "directory entries",
        ),
        (vec!["session", "list", "--workspace", "latest"], "Sessions"),
    ] {
        let mut args = args;
        args.extend(["--organization", "selected"]);
        let shown = client::ran_on_a_terminal(&operator, &args, 160, "");
        assert!(shown.status.success(), "{}", shown.said);
        assert!(
            shown
                .said
                .contains(&format!("No {absent} in Organization selected.")),
            "{}",
            shown.said
        );
        assert!(shown.said.contains("latest"), "{}", shown.said);
        assert!(shown.said.contains(&operator), "{}", shown.said);
        let piped = client::ran(&operator, &args);
        assert!(piped.status.success(), "{}", piped.err);
        assert!(piped.out.is_empty(), "{:?}", piped.out);
        assert!(piped.err.is_empty(), "{}", piped.err);
        args.push("--json");
        let json = client::ran(&operator, &args);
        assert!(json.status.success(), "{}", json.err);
        assert!(!json.out.join("\n").contains("No "));
        if args[0] == "workspace" {
            assert!(json.json().is_object(), "{:?}", json.out);
        } else {
            assert_eq!(json.json(), serde_json::json!([]));
        }
    }
    let args = [
        "workspace",
        "commits",
        "latest",
        "--organization",
        "selected",
    ];
    let shown = client::ran_on_a_terminal(&operator, &args, 160, "");
    assert!(shown.status.success(), "{}", shown.said);
    assert!(shown.said.contains("repo:"), "{}", shown.said);
    let piped = client::ran(&operator, &args);
    assert!(piped.status.success(), "{}", piped.err);
    assert_eq!(piped.out, ["abcd1234 a local commit"]);
    let args = ["workspace", "work", "latest", "--organization", "selected"];
    let shown = client::ran_on_a_terminal(&operator, &args, 160, "");
    assert!(shown.status.success(), "{}", shown.said);
    assert!(
        shown.said.contains("Reported by the supervisor"),
        "{}",
        shown.said
    );
    let piped = client::ran(&operator, &args);
    assert!(piped.status.success(), "{}", piped.err);
    assert_eq!(piped.out, ["  checkout unavailable"]);
    serving.join().expect("the stub served every read");
}
