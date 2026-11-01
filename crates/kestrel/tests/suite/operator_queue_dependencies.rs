//! Sessions that depend on others: opening, enqueueing and refusing them, and what a blocker's
//! end does to them.

use crate::support;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::fixture::Fixture;
use support::operator_queue::{numbered, queue_of};
use support::operator_requests::{
    an_open_workspace, client, declared, got, listed, recorded, session_at, sessions_of,
};

#[tokio::test]
async fn an_operator_opens_a_workspace_whose_session_waits_on_another() {
    let kestrel = Kestrel::boot().await;
    let (_, blocker) = an_open_workspace(&kestrel, 0).await;

    let (status, opened) = declared(
        &kestrel,
        &operator::WORKSPACES.replace("{organization}", "acme"),
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "brief": "Carry on once the other one lands",
            "depends_on": [blocker.name],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    assert_eq!(
        opened["session"]["depends_on"],
        json!([{ "id": blocker.id.to_string(), "name": blocker.name }])
    );

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let waiting = numbered(&queue)
        .into_iter()
        .find(|row| row["name"] == opened["session"]["name"])
        .expect("the dependent Session is queued");
    assert_eq!(waiting["position"], Value::Null);
    assert_eq!(
        waiting["reasons"],
        json!([{ "kind": "dependencies", "sessions": [blocker.name] }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_operator_enqueues_a_session_on_blockers_one_of_which_has_already_succeeded() {
    let kestrel = Kestrel::boot().await;
    let (_, working) = an_open_workspace(&kestrel, 0).await;
    let succeeded = kestrel
        .dispatch_session(
            kestrel
                .open_workspace("acme", "kestrel", "builder")
                .await
                .id,
        )
        .await;
    kestrel.complete_session(&succeeded).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let earlier = kestrel.dispatch_session(workspace.id).await;
    kestrel.complete_session(&earlier).await;

    let (status, enqueued) = declared(
        &kestrel,
        &sessions_of("acme", &workspace.id.to_string()),
        &json!({ "depends_on": [succeeded.name, working.name, working.id.to_string()] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{enqueued}");
    let mut depends_on = enqueued["depends_on"]
        .as_array()
        .expect("the declared blockers")
        .iter()
        .map(|blocker| blocker["name"].as_str().expect("a name").to_owned())
        .collect::<Vec<_>>();
    depends_on.sort();
    let mut expected = vec![succeeded.name.clone(), working.name.clone()];
    expected.sort();
    assert_eq!(depends_on, expected, "a duplicate blocker is one edge");

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let waiting = numbered(&queue)
        .into_iter()
        .find(|row| row["name"] == enqueued["name"])
        .expect("the dependent Session is queued");
    assert_eq!(
        waiting["reasons"],
        json!([{ "kind": "dependencies", "sessions": [working.name] }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dependency_no_session_could_meet_is_refused_and_leaves_nothing_behind() {
    let kestrel = Kestrel::boot().await;
    let (_, working) = an_open_workspace(&kestrel, 0).await;
    let failed = kestrel
        .dispatch_session(
            kestrel
                .open_workspace("acme", "kestrel", "builder")
                .await
                .id,
        )
        .await;
    kestrel
        .fail_session(&failed, "the agent could not open a pull request")
        .await;
    let fails = kestrel
        .dispatch_session(
            kestrel
                .open_workspace("acme", "kestrel", "builder")
                .await
                .id,
        )
        .await;
    let unreachable = kestrel
        .enqueue_session(
            kestrel
                .open_workspace("acme", "kestrel", "builder")
                .await
                .id,
        )
        .await;
    kestrel.block_session(&unreachable, &fails).await;
    kestrel
        .fail_session(&fails, "the agent could not open a pull request")
        .await;
    Fixture::acme()
        .organization("globex")
        .declare(&kestrel)
        .await;
    let foreign = kestrel
        .enqueue_session(
            kestrel
                .open_workspace("globex", "kestrel", "builder")
                .await
                .id,
        )
        .await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");
    let opened_before = listed(&kestrel, &workspaces).await.len();
    let (_, queue) = got(&kestrel, &queue_of("acme")).await;
    let queued_before = numbered(&queue).len();

    let failed_id = failed.id.to_string();
    for (blocker, refused, why) in [
        ("never-was-ffffffff", StatusCode::NOT_FOUND, "no session"),
        (foreign.name.as_str(), StatusCode::NOT_FOUND, "no session"),
        (failed_id.as_str(), StatusCode::CONFLICT, "failed"),
        (
            unreachable.name.as_str(),
            StatusCode::CONFLICT,
            "unreachable",
        ),
    ] {
        let (status, said) = declared(
            &kestrel,
            &workspaces,
            &json!({
                "project": "kestrel",
                "agent": "builder",
                "brief": "Carry on once the other one lands",
                "depends_on": [working.name, blocker],
            }),
        )
        .await;
        assert_eq!(status, refused, "{said}");
        let said = said.to_string();
        assert!(
            said.contains(blocker) && said.contains(why),
            "the refusal of {blocker} does not say it is {why}: {said}"
        );
    }

    assert_eq!(
        listed(&kestrel, &workspaces).await.len(),
        opened_before,
        "a refused open left a Workspace behind"
    );
    let (_, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(
        numbered(&queue).len(),
        queued_before,
        "a refused open left a Session behind"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dependent_takes_its_original_place_once_its_blocker_succeeds() {
    let kestrel = Kestrel::boot().await;
    kestrel.record_dispatch(8, "local-exec").await;
    let (_, blocker) = an_open_workspace(&kestrel, 0).await;
    let (status, opened) = declared(
        &kestrel,
        &operator::WORKSPACES.replace("{organization}", "acme"),
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "brief": "Carry on once the other one lands",
            "depends_on": [blocker.name],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let behind = kestrel
        .enqueue_session(
            kestrel
                .open_workspace("acme", "kestrel", "builder")
                .await
                .id,
        )
        .await;

    kestrel.complete_session(&blocker).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| (row["name"].clone(), row["position"].clone()))
            .collect::<Vec<_>>(),
        [
            (opened["session"]["name"].clone(), json!(1)),
            (json!(behind.name), json!(2)),
        ]
    );
    assert_eq!(
        kestrel
            .claim_session()
            .await
            .map(|claimed| claimed.id.to_string()),
        opened["session"]["id"].as_str().map(str::to_owned),
        "the dependent was not dispatched first once its blocker succeeded"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_whose_blocker_fails_is_unreachable_and_names_what_it_waited_on() {
    let kestrel = Kestrel::boot().await;
    let (_, blocker) = an_open_workspace(&kestrel, 0).await;
    let (status, opened) = declared(
        &kestrel,
        &operator::WORKSPACES.replace("{organization}", "acme"),
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "depends_on": [blocker.name],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");

    kestrel
        .fail_session(&blocker, "the agent could not open a pull request")
        .await;

    let (status, dependent) = got(
        &kestrel,
        &session_at(
            "acme",
            opened["session"]["name"]
                .as_str()
                .expect("a generated name"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dependent}");
    assert_eq!(dependent["state"], "unreachable");
    assert_eq!(
        dependent["depends_on"],
        json!([{ "id": blocker.id.to_string(), "name": blocker.name }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_opens_and_enqueues_sessions_that_depend_on_others() {
    let kestrel = Kestrel::boot().await;
    let (_, blocker) = an_open_workspace(&kestrel, 0).await;
    let other = kestrel
        .dispatch_session(
            kestrel
                .open_workspace("acme", "kestrel", "builder")
                .await
                .id,
        )
        .await;

    let opened = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--depends-on",
                &blocker.name,
                "--depends-on",
                &other.name,
                "--json",
            ],
        )
        .await,
    );
    let dependent = opened[0]["session"]["name"]
        .as_str()
        .expect("a generated name");
    let shown = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "show",
                dependent,
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    let mut depends_on = shown[0]["depends_on"]
        .as_array()
        .expect("the declared blockers")
        .iter()
        .map(|blocker| blocker["name"].as_str().expect("a name").to_owned())
        .collect::<Vec<_>>();
    depends_on.sort();
    let mut expected = vec![blocker.name.clone(), other.name.clone()];
    expected.sort();
    assert_eq!(depends_on, expected);

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let earlier = kestrel.dispatch_session(workspace.id).await;
    kestrel.complete_session(&earlier).await;
    let enqueued = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "enqueue",
                "--organization",
                "acme",
                "--workspace",
                &workspace.id.to_string(),
                "--depends-on",
                &blocker.name,
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(
        enqueued[0]["depends_on"],
        json!([{ "id": blocker.id.to_string(), "name": blocker.name }])
    );

    kestrel.teardown().await;
}
