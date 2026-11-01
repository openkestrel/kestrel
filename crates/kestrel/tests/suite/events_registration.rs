use std::time::Duration;

use kestrel::domain::Direction;

use crate::support::Kestrel;
use crate::support::events::{
    BOTH, PATIENCE, REPOSITORY, eagerly, labelled_ready, polled, polls, recorded, watching,
};
use crate::support::github_stub::GithubStub;

#[tokio::test]
async fn an_integration_declares_which_directions_it_carries() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    watching(&kestrel, &stub, &[Direction::Inbound]).await;

    let registered = kestrel.integrations("acme").await;

    assert_eq!(registered.len(), 1);
    assert!(registered[0].carries(Direction::Inbound));
    assert!(!registered[0].carries(Direction::Outbound));

    kestrel.teardown().await;
}

/// The direction is what an Integration does rather than a label beside it: nothing polls one
/// that only carries kestrel's requests outward.
#[tokio::test]
async fn an_integration_that_carries_only_outbound_is_never_polled() {
    let outbound_only = GithubStub::start();
    let polling = GithubStub::start();
    let kestrel = Kestrel::boot().await;

    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "outbound",
            REPOSITORY,
            &outbound_only.base_url(),
            &[Direction::Outbound],
            eagerly(),
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "inbound",
            REPOSITORY,
            &polling.base_url(),
            &[Direction::Inbound],
            eagerly(),
        )
        .await;

    polled(&polling, 3).await;

    assert_eq!(
        polls(&outbound_only),
        0,
        "an integration that carries nothing inbound was polled"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integration_names_a_repository_as_owner_and_name() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = kestrel
        .try_register_integration(
            "acme",
            "github",
            "kestrel",
            &stub.base_url(),
            BOTH,
            eagerly(),
        )
        .await
        .expect_err("a repository that is not owner/name should be refused");

    assert!(
        refusal.to_string().contains("owner/name"),
        "unhelpful refusal: {refusal}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integration_and_what_it_discovers_belong_to_one_organization() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;
    kestrel.declare_organization("globex").await;

    recorded(&kestrel, 1).await;

    assert!(
        kestrel.integrations("globex").await.is_empty(),
        "another organization can see the integration acme registered"
    );
    assert!(
        kestrel.events("globex").await.is_empty(),
        "another organization can see the events acme's integration discovered"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_organizations_keep_separate_copies_of_the_same_producer_event() {
    let acme = GithubStub::start();
    let globex = GithubStub::start();
    acme.deliver(labelled_ready(43));
    globex.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel.declare_organization("globex").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &acme.base_url(),
            BOTH,
            eagerly(),
        )
        .await;
    kestrel
        .register_integration(
            "globex",
            "github",
            REPOSITORY,
            &globex.base_url(),
            BOTH,
            eagerly(),
        )
        .await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let acme_events = kestrel.events("acme").await;
        let globex_events = kestrel.events("globex").await;
        if acme_events.len() == 1 && globex_events.len() == 1 {
            assert_eq!(acme_events[0].occurrence.id, globex_events[0].occurrence.id);
            assert_ne!(acme_events[0].record_id, globex_events[0].record_id);
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "both organizations did not retain their own Event"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    kestrel.teardown().await;
}
