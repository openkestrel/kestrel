//! What the supervisor does once it has a link to dial is proved against a real control plane
//! by the primary test seam, in the kestrel crate.

use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn the_supervisor_binary_starts_and_says_it_has_no_link_to_dial() {
    let supervisor = Command::new(env!("CARGO_BIN_EXE_kestrel-supervisor"))
        .env_clear()
        .output()
        .expect("the supervisor should spawn");

    let said = String::from_utf8_lossy(&supervisor.stderr);

    assert_eq!(
        supervisor.status.code(),
        Some(1),
        "the supervisor exited {}. it said:\n{said}",
        supervisor.status
    );
    assert!(said.contains("supervisor started"), "it said:\n{said}");
    assert!(said.contains("no link to dial"), "it said:\n{said}");
}

/// No Session is carried, so nothing lapses: the supervisor stays for as long as its Instance does,
/// redialling a control plane that may yet come back.
#[test]
fn a_supervisor_carrying_no_session_keeps_redialling_a_link_that_is_gone() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port to leave nothing listening on");
    let port = listener.local_addr().expect("a bound address").port();
    drop(listener);

    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_kestrel-supervisor"))
        .env_clear()
        .env("KESTREL_LINK", format!("http://127.0.0.1:{port}"))
        .env("KESTREL_INSTANCE", "local-exec/kestrel-an-instance")
        .env("KESTREL_INSTANCE_CREDENTIAL", "a-credential")
        .env("KESTREL_LEASE", "1")
        .stderr(Stdio::piped())
        .spawn()
        .expect("the supervisor should spawn");

    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        assert!(
            supervisor
                .try_wait()
                .expect("the supervisor should be waitable")
                .is_none(),
            "the supervisor exited with no session to give up on"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    supervisor
        .kill()
        .expect("the supervisor should be killable");
    let said = supervisor
        .wait_with_output()
        .expect("the supervisor should be reaped");
    let said = String::from_utf8_lossy(&said.stderr);

    assert!(
        said.matches("lost the link").count() > 1,
        "it said:\n{said}"
    );
    assert!(!said.contains("gave up"), "it said:\n{said}");
}
