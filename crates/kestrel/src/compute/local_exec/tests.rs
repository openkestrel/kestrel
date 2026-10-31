use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

use tempfile::TempDir;

use super::super::{Driver, Exited, Instance, Supervisor};
use super::LocalExec;
use crate::domain::SessionId;
use crate::timer::Clock;

fn process_exists(pid: i32) -> bool {
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(pid, 0) == 0
    }
}

fn eventually_gone(pid: i32) {
    let deadline = Clock::System.instant().into_std() + Duration::from_secs(5);
    while Clock::System.instant().into_std() < deadline {
        if !process_exists(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("pid {pid} is still alive 5s after its tree should have been stopped");
}

fn driver(scripts: &TempDir, shell: &str) -> Driver {
    let script = scripts.path().join("supervisor");
    fs::write(&script, format!("#!/bin/sh\n{shell}\n")).expect("a script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("an executable script");

    Driver::LocalExec(LocalExec::running(&script))
}

fn provisioned(driver: &Driver) -> Instance {
    driver
        .provision(SessionId::generate())
        .expect("the instance should provision")
}

/// A shell that backgrounds a grandchild `sleep` and prints its pid, so the test can prove
/// the whole tree died rather than only the process the supervisor handle holds directly.
fn a_tree_with_a_grandchild(instance: &mut Instance) -> (Supervisor, i32) {
    let mut supervisor = instance
        .supervise(&[])
        .expect("the supervisor should start");

    let stdout = supervisor.take_stdout().expect("stdout should be piped");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("the grandchild's pid should print");
    let grandchild: i32 = line.trim().parse().expect("a pid");

    (supervisor, grandchild)
}

#[test]
fn stopping_a_supervisor_leaves_no_orphan_process_in_its_tree() {
    let scripts = TempDir::new().expect("a temporary directory");
    let driver = driver(&scripts, "sleep 30 & echo $!\nwait");
    let mut instance = provisioned(&driver);
    let (supervisor, grandchild) = a_tree_with_a_grandchild(&mut instance);

    supervisor.stop().expect("stop should succeed");

    eventually_gone(grandchild);
    instance.destroy().expect("destroy should succeed");
}

#[test]
fn a_supervisor_outlives_its_handle_and_goes_with_its_instance() {
    let scripts = TempDir::new().expect("a temporary directory");
    let driver = driver(&scripts, "sleep 30 & echo $!\nwait");
    let mut instance = provisioned(&driver);
    let grandchild = {
        let (supervisor, grandchild) = a_tree_with_a_grandchild(&mut instance);
        drop(supervisor);
        grandchild
    };
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        process_exists(grandchild),
        "dropping the handle stopped the supervisor"
    );

    instance.destroy().expect("destroy should succeed");

    eventually_gone(grandchild);
}

#[test]
fn a_supervisor_stopped_by_name_takes_its_tree_with_it() {
    let scripts = TempDir::new().expect("a temporary directory");
    let driver = driver(&scripts, "sleep 30 & echo $!\nwait");
    let mut instance = provisioned(&driver);
    let (supervisor, grandchild) = a_tree_with_a_grandchild(&mut instance);

    driver
        .stop_named(supervisor.name())
        .expect("stop should succeed");

    eventually_gone(grandchild);
    drop(supervisor);
    instance.destroy().expect("destroy should succeed");
}

#[test]
fn what_a_session_wrote_outlives_its_supervisor_and_goes_with_the_instance() {
    let scripts = TempDir::new().expect("a temporary directory");
    let driver = driver(&scripts, "echo what an agent left behind > left");
    let mut instance = provisioned(&driver);
    let name = instance.name().to_owned();

    let mut supervisor = instance
        .supervise(&[])
        .expect("the supervisor should start");
    while supervisor
        .status()
        .expect("the status should read")
        .is_none()
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    supervisor.stop().expect("stop should succeed");

    let mut resumed = driver
        .resume(&name)
        .expect("the instance should resume")
        .expect("the instance should still be there");
    assert_eq!(
        resumed.read_file("left").expect("the file should read"),
        b"what an agent left behind\n"
    );

    resumed.destroy().expect("destroy should succeed");
    assert!(
        driver
            .resume(&name)
            .expect("a gone instance is not an error")
            .is_none(),
        "the instance outlived being destroyed"
    );
}

#[test]
fn a_command_execs_in_the_root_and_streams_what_it_says() {
    let scripts = TempDir::new().expect("a temporary directory");
    let mut instance = provisioned(&driver(&scripts, "sleep 30"));
    instance
        .write_file("read-me", b"in the root")
        .expect("the file should write");

    let finished = instance
        .exec(&["cat", "read-me"])
        .expect("cat should exec")
        .finish()
        .expect("cat should finish");

    assert!(finished.exited.success(), "cat said {finished:?}");
    assert_eq!(finished.out, "in the root");

    instance.destroy().expect("destroy should succeed");
}

#[test]
fn a_supervisor_that_is_still_running_has_no_status_and_one_that_ended_has_the_code() {
    let scripts = TempDir::new().expect("a temporary directory");
    let mut instance = provisioned(&driver(&scripts, "exit 3"));
    let mut supervisor = instance
        .supervise(&[])
        .expect("the supervisor should start");

    let deadline = Clock::System.instant().into_std() + Duration::from_secs(5);
    loop {
        match supervisor.status().expect("the status should read") {
            Some(exited) => {
                assert_eq!(exited, Exited::with(3));
                break;
            }
            None => assert!(
                Clock::System.instant().into_std() < deadline,
                "the supervisor never exited"
            ),
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    supervisor.stop().expect("stop should succeed");
    instance.destroy().expect("destroy should succeed");
}
