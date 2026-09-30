//! The Docker `Compute` driver: the Session the primary test seam already drives, executed in a
//! real container provisioned from the `kestrel-env` image.
//!
//! Every test here builds and runs images, which a `cargo test` has no business doing on its
//! own, so they are ignored by default and CI runs them with `--ignored`.

mod support;

use std::time::Duration;

use kestrel::compute::{Docker, Driver};
use kestrel::domain::{Exit, Session, SessionId, Workspace, WorkspaceId};
use support::Kestrel;
use support::image::{self, Container};
use support::scripted_agent::{self, Script};

const PATIENCE: Duration = Duration::from_secs(120);

/// A repository the container can reach, which one on this machine is not.
const REPOSITORY: &str = "https://github.com/jtmthf/kestrel";
const BRANCH: &str = "main";

async fn working(script: Script) -> Kestrel {
    Kestrel::dispatching_in(
        image::with_the_scripted_agent(),
        &scripted_agent::playing_in_an_image(script),
    )
    .await
}

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[REPOSITORY.to_owned()], BRANCH)
        .await;
    kestrel
        .declare_agent(
            &organization,
            "builder",
            "opencode",
            Some(kestrel_scripted_agent::OTHER_MODEL),
        )
        .await;

    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;

    kestrel.open_workspace("acme", "kestrel", "builder").await
}

async fn until(
    kestrel: &Kestrel,
    session: SessionId,
    what: &str,
    ready: impl Fn(&Session) -> bool,
) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let session = kestrel.session(session).await;
        if ready(&session) {
            return session;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} is {} with the exit status {:?}, and never {what}",
            session.id,
            session.state,
            session.exit
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Answering a turn never ends a Session, so one that answered is stopped, the way a person would.
async fn ended(kestrel: &Kestrel, session: SessionId) -> Session {
    kestrel.after_one_turn_within(session, PATIENCE).await
}

/// A stopped Session's exit is recorded before its supervisor has ended its harness (ADR-0024), so
/// the container is given a moment to catch up before this looks for what the Session left behind.
async fn without_its_harness(container: &Container) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let left = container.processes();
        let clean = !left.contains("kestrel-scripted-agent");
        if clean || tokio::time::Instant::now() >= deadline {
            return left;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn started(kestrel: &Kestrel, session: SessionId) -> Session {
    until(kestrel, session, "started", |session| {
        session.started_at.is_some()
    })
    .await
}

async fn enqueue_when_free(kestrel: &Kestrel, workspace: WorkspaceId) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        match kestrel.try_enqueue_session(workspace).await {
            Ok(session) => return session,
            Err(error) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the session should enqueue: {error}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// The Session of ticket 06, dispatched at the driver the domain never names: the same script, the
/// same transcript, the same exit.
#[tokio::test]
#[ignore = "builds and runs the kestrel-env image"]
async fn the_scripted_session_ends_the_same_way_in_a_container_as_it_does_in_a_process() {
    let kestrel = working(Script::Speaks).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    assert_eq!(ended.exit, Some(Exit::Succeeded));
    assert_eq!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .map(|entry| entry.entry.to_string())
            .filter(|entry| entry.starts_with("said  builder"))
            .collect::<Vec<_>>(),
        vec![
            "said  builder  half of one message, and the other half".to_owned(),
            "said  builder  a second message".to_owned(),
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
#[ignore = "builds and runs the kestrel-env image"]
async fn an_instance_is_a_container_whose_supervisor_outlives_its_session_and_goes_with_it() {
    let kestrel = working(Script::Speaks).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    let instance = ended.instance.as_deref().expect("an instance");
    assert_eq!(
        instance,
        format!("docker/kestrel-{}", session.id),
        "a session names the container it executed on"
    );
    let container = Container::named(instance);
    let left = without_its_harness(&container).await;
    assert!(
        !left.contains("kestrel-scripted-agent"),
        "the session left its harness on its instance: {left}"
    );
    assert!(
        left.contains("kestrel-supervisor"),
        "the supervisor went with its session: {left}"
    );

    kestrel.release_instance(workspace.id).await;
    container.is_gone().await;

    kestrel.teardown().await;
}

#[tokio::test]
#[ignore = "builds and runs the kestrel-env image"]
async fn an_instance_reaps_a_grandchild_and_stays_running() {
    let driver = Driver::Docker(Docker::provisioning_from(image::built()));
    let mut instance = driver
        .provision(SessionId::generate())
        .expect("the instance should provision");
    let child = instance
        .exec(&["sh", "-c", "sleep infinity >/dev/null 2>&1 & echo $!"])
        .expect("the shell should start")
        .finish()
        .expect("the shell should finish");
    assert!(child.exited.success(), "the shell said {child:?}");
    let pid = child.out.trim();
    let killed = instance
        .exec(&["sh", "-c", "kill -9 \"$1\"", "sh", pid])
        .expect("the grandchild should be signalled")
        .finish()
        .expect("the signal should finish");
    assert!(killed.exited.success(), "the signal said {killed:?}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let reaped = instance
            .exec(&["sh", "-c", "test ! -e /proc/$1", "sh", pid])
            .expect("the instance should still accept commands")
            .finish()
            .expect("the command should finish");
        if reaped.exited.success() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{pid} was not reaped"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    instance.destroy().expect("the instance should destroy");
}

#[tokio::test]
#[ignore = "builds and runs the kestrel-env image"]
async fn a_projects_repositories_and_its_branch_are_in_the_container() {
    let kestrel = working(Script::Dawdles).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let container = Container::named(
        started(&kestrel, session.id)
            .await
            .instance
            .as_deref()
            .expect("an instance"),
    );

    let branch = container.exec(&[
        "git",
        "-C",
        "/workspace/kestrel",
        "branch",
        "--show-current",
    ]);
    assert_eq!(
        branch.out, workspace.checkout.branch,
        "the checkout is not on its workspace's branch: {branch:?}"
    );
    let readme = container.exec(&["test", "-f", "/workspace/kestrel/README.md"]);
    assert_eq!(
        readme.code, 0,
        "the repository is not on the instance: {readme:?}"
    );

    kestrel.teardown().await;
    container.is_gone().await;
}

/// A container that dies takes the supervisor holding the Session's lease out with it, so the
/// Session cannot go on; the work role sees its supervisor gone before the lease it stopped holding
/// out is due, and that is what ends it.
#[tokio::test]
#[ignore = "builds and runs the kestrel-env image"]
async fn a_container_that_dies_mid_session_is_detected_and_the_next_session_starts_it_again() {
    let kestrel = working(Script::Dawdles).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let container = Container::named(
        started(&kestrel, session.id)
            .await
            .instance
            .as_deref()
            .expect("an instance"),
    );

    container.kill();

    let ended = ended(&kestrel, session.id).await;
    let Some(Exit::Failed { because }) = &ended.exit else {
        panic!(
            "the session ended {:?}, and its container was killed",
            ended.exit
        );
    };
    assert!(
        because.contains("without reporting how the session went"),
        "unhelpful exit status: {because}"
    );
    assert!(
        ended.lease_expires_at.is_none(),
        "a session whose container died still holds a lease"
    );

    let next = enqueue_when_free(&kestrel, workspace.id).await;
    let next = started(&kestrel, next.id).await;
    assert_eq!(
        next.instance, ended.instance,
        "a stopped container was taken for gone"
    );

    kestrel.teardown().await;
    container.is_gone().await;
}

/// Every operation against a real container, including the ones no Session makes: a
/// driver that implemented only what the work role happens to reach for would be a driver that
/// has to grow to meet the contract later.
#[tokio::test]
#[ignore = "builds and runs the kestrel-env image"]
async fn every_operation_in_the_contract_works_against_a_container() {
    let kestrel = Kestrel::boot_reachable_from_an_environment().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    // Provisioned through the port rather than through the work role, so the operations no
    // Session makes are exercised on the same Instance as the ones it does.
    let driver = Driver::Docker(Docker::provisioning_from(image::built()));
    let mut instance = driver
        .provision(session.id)
        .expect("the instance should provision");
    let container = Container::named(instance.name());
    let on = kestrel.on_the_link_at(&session, instance.name()).await;
    let mut supervisor = instance
        .supervise(&[
            ("KESTREL_LINK", &kestrel.link_from_an_environment()),
            ("KESTREL_INSTANCE", &on.instance),
            ("KESTREL_INSTANCE_CREDENTIAL", on.credential.as_str()),
        ])
        .expect("the supervisor should start");

    assert_eq!(
        supervisor.status().expect("the status should read"),
        None,
        "a supervisor that was just started has already ended"
    );
    driver
        .stop_named(supervisor.name())
        .expect("the supervisor should stop");
    supervisor.stop().expect("the supervisor should stop");
    assert!(
        container
            .exec(&["sh", "-c", "env | grep KESTREL_INSTANCE_CREDENTIAL"])
            .code
            != 0,
        "the instance's credential outlived its supervisor"
    );

    let mut instance = driver
        .resume(instance.name())
        .expect("the instance should resume")
        .expect("the instance should still be there");
    instance
        .write_file("wrote/file", b"from outside the container")
        .expect("the file should write");
    assert_eq!(
        instance.read_file("wrote/file").expect("a read"),
        b"from outside the container"
    );

    let listed = instance
        .exec(&["ls", "wrote"])
        .expect("ls should exec")
        .finish()
        .expect("ls should finish");
    assert!(listed.exited.success(), "ls said {listed:?}");
    assert_eq!(listed.out, "file");

    let name = instance.name().to_owned();
    instance.destroy().expect("the container should destroy");
    container.is_gone().await;
    assert!(
        driver
            .resume(&name)
            .expect("a gone container is not an error")
            .is_none()
    );

    kestrel.teardown().await;
}
