//! `git` and `gh` reach GitHub through the broker as a real `git` and a real process run them,
//! against a stub link standing in for the control plane.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use kestrel_supervisor::broker::Broker;
use kestrel_supervisor::link::Link;
use tokio::sync::watch;

const TOKEN: &str = "ghs_lent_for_one_use";
const ASKED: &str = "protocol=https\nhost=github.com\npath=jtmthf/kestrel.git\n\n";

struct Stub {
    port: u16,
    asked: Arc<Mutex<Vec<String>>>,
    _serving: JoinHandle<()>,
}

impl Stub {
    fn answering(status: u16, body: &str) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("the stub should bind a port");
        let port = server.server_addr().to_ip().expect("bound over IP").port();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let serving = {
            let asked = Arc::clone(&asked);
            let body = body.to_owned();
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    asked
                        .lock()
                        .expect("the request log")
                        .push(request.url().to_owned());
                    let _ = request.respond(
                        tiny_http::Response::from_string(&body)
                            .with_status_code(status)
                            .with_header(
                                tiny_http::Header::from_bytes("content-type", "application/json")
                                    .expect("a header"),
                            ),
                    );
                }
            })
        };

        Self {
            port,
            asked,
            _serving: serving,
        }
    }

    fn lending() -> Self {
        Self::answering(
            200,
            &serde_json::json!({ "token": TOKEN, "expires_at": "2099-01-01T00:00:00Z" })
                .to_string(),
        )
    }

    fn link(&self) -> Arc<Link> {
        Arc::new(Link::to(
            &format!("http://127.0.0.1:{}", self.port),
            "local-exec/an-instance",
            "a-credential",
        ))
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("the request log").clone()
    }
}

fn opened(stub: &Stub, carried: Option<&str>) -> (Broker, watch::Sender<Option<String>>) {
    let (carrying, session) = watch::channel(carried.map(str::to_owned));
    let broker = Broker::open(
        stub.link(),
        session,
        Path::new(env!("CARGO_BIN_EXE_kestrel-supervisor")),
        &BTreeMap::new(),
    )
    .expect("the broker should open");

    (broker, carrying)
}

/// Only what the broker hands it, so a credential this machine holds cannot answer instead.
fn hermetic(command: &mut Command, broker: &Broker) {
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", std::env::temp_dir())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .envs(broker.environment());
}

async fn filled(broker: &Broker) -> Output {
    let mut git = Command::new("git");
    git.args(["credential", "fill"]);
    hermetic(&mut git, broker);

    tokio::task::spawn_blocking(move || {
        let mut running = git
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("git should run");
        running
            .stdin
            .take()
            .expect("a stdin")
            .write_all(ASKED.as_bytes())
            .expect("git should be asked");
        running.wait_with_output().expect("git should finish")
    })
    .await
    .expect("git should be waited on")
}

#[tokio::test]
async fn git_is_handed_the_token_the_link_lends_for_the_repository_it_reaches() {
    let stub = Stub::lending();
    let (broker, _carrying) = opened(&stub, Some("a-session"));

    let filled = filled(&broker).await;

    assert!(
        filled.status.success(),
        "{}",
        String::from_utf8_lossy(&filled.stderr)
    );
    let said = String::from_utf8_lossy(&filled.stdout);
    assert!(said.contains("username=x-access-token\n"), "{said}");
    assert!(said.contains(&format!("password={TOKEN}\n")), "{said}");
    assert_eq!(
        stub.asked(),
        [
            "/link/instances/local-exec%2Fan-instance/github/token?session=a-session&repository=https%3A%2F%2Fgithub%2Ecom%2Fjtmthf%2Fkestrel%2Egit"
        ]
    );
}

#[tokio::test]
async fn a_refusal_stops_git_with_the_reason_and_lends_it_nothing_from_elsewhere() {
    let stub = Stub::answering(
        409,
        r#"{"message":"the integration github is disabled; enable it to resume what waits on it"}"#,
    );
    let (broker, _carrying) = opened(&stub, Some("a-session"));

    let filled = filled(&broker).await;

    assert!(!filled.status.success());
    assert!(!String::from_utf8_lossy(&filled.stdout).contains("password="));
    assert!(
        String::from_utf8_lossy(&filled.stderr)
            .contains("kestrel: the integration github is disabled"),
        "{}",
        String::from_utf8_lossy(&filled.stderr)
    );
}

#[tokio::test]
async fn an_instance_carrying_no_session_lends_nothing_and_asks_the_link_nothing() {
    let stub = Stub::lending();
    let (broker, _carrying) = opened(&stub, None);

    let filled = filled(&broker).await;

    assert!(!filled.status.success());
    assert!(String::from_utf8_lossy(&filled.stderr).contains("carrying no session"));
    assert!(stub.asked().is_empty());
}

#[tokio::test]
async fn gh_is_run_with_the_token_in_its_own_environment_for_the_repository_it_names() {
    let stub = Stub::lending();
    let (broker, _carrying) = opened(&stub, Some("a-session"));
    let held = tempfile::TempDir::new().expect("a directory");
    let real = held.path().join("gh");
    let saw = held.path().join("saw");
    std::fs::write(
        &real,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$GH_TOKEN\" \"$@\" > '{}'\n",
            saw.display()
        ),
    )
    .expect("a stand-in gh");
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).expect("an executable");
    let mut shim = Command::new(env!("CARGO_BIN_EXE_kestrel-supervisor"));
    shim.arg("gh")
        .arg(&real)
        .args(["pr", "create", "-R", "jtmthf/kestrel", "--fill"])
        .current_dir(held.path());
    hermetic(&mut shim, &broker);

    let ran = tokio::task::spawn_blocking(move || shim.output().expect("the shim should run"))
        .await
        .expect("the shim should be waited on");

    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&saw).expect("gh ran"),
        format!("{TOKEN}\npr\ncreate\n-R\njtmthf/kestrel\n--fill\n")
    );
    assert_eq!(
        stub.asked(),
        [
            "/link/instances/local-exec%2Fan-instance/github/token?session=a-session&repository=https%3A%2F%2Fgithub%2Ecom%2Fjtmthf%2Fkestrel"
        ]
    );
    assert!(
        !ran.stdout
            .windows(TOKEN.len())
            .any(|seen| seen == TOKEN.as_bytes())
    );
}
