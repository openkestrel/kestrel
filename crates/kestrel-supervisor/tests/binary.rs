//! What the supervisor does once it has a link to dial is proved against a real control plane
//! by the primary test seam, in the kestrel crate.

use std::net::TcpListener;
use std::process::Command;
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

/// A control plane that is gone for good never answers, and the supervisor was told how long the
/// Session's lease is held out for: past that, it stops rather than reconnecting forever.
#[test]
fn the_supervisor_gives_up_when_the_link_is_gone_for_good() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port to leave nothing listening on");
    let port = listener.local_addr().expect("a bound address").port();
    drop(listener);

    let started = Instant::now();
    let supervisor = Command::new(env!("CARGO_BIN_EXE_kestrel-supervisor"))
        .env_clear()
        .env("KESTREL_LINK", format!("http://127.0.0.1:{port}"))
        .env("KESTREL_SESSION", "01999cf2-0000-7000-8000-000000000000")
        .env("KESTREL_SESSION_CREDENTIAL", "a-credential")
        .env("KESTREL_HARNESS_COMMAND", "unused")
        .env("KESTREL_LEASE", "1")
        .output()
        .expect("the supervisor should spawn");

    let said = String::from_utf8_lossy(&supervisor.stderr);
    assert_eq!(
        supervisor.status.code(),
        Some(1),
        "the supervisor exited {}. it said:\n{said}",
        supervisor.status
    );
    assert!(said.contains("gave up"), "it said:\n{said}");
    assert!(said.contains("lease"), "it said:\n{said}");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the supervisor took {:?} to give up",
        started.elapsed()
    );
}
