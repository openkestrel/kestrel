use std::cell::Cell;

use anyhow::anyhow;

use super::*;

fn test_in(state: &str, outcome: Option<&str>) -> wire::SignInTest {
    serde_json::from_value(json!({
        "id": "t1", "harness": "claude", "method": "claude-setup-token", "revision": 3,
        "requested_model": null, "model": null, "image": null, "state": state,
        "outcome": outcome, "message": null, "created_at": "2026-10-31T00:00:00Z",
        "started_at": null, "finished_at": null,
    }))
    .expect("a test")
}

fn method(providers: Option<&[&str]>) -> wire::SignInMethod {
    serde_json::from_value(json!({
        "id": "opencode-go-zen", "name": "OpenCode Go or Zen", "kind": "subscription",
        "ownership": "operator", "input": "token", "test_providers": providers,
        "fills": { "kind": "variable", "variable": "OPENCODE_API_KEY" },
    }))
    .expect("a method")
}

fn lost() -> anyhow::Error {
    anyhow!(Failed::new(Exit::Unavailable, "no answer"))
}

#[tokio::test]
async fn a_lost_answer_reads_the_known_test_again_until_it_ends() {
    let reads = Cell::new(0);
    let answers = [Err(lost()), Err(lost()), Ok(test_in("running", None))]
        .into_iter()
        .chain([Ok(test_in("completed", Some("worked")))]);
    let answers = std::cell::RefCell::new(answers);

    let ended = awaited(
        test_in("pending", None),
        || {
            reads.set(reads.get() + 1);
            let answer = answers.borrow_mut().next().expect("no read after the end");
            async move { answer }
        },
        Duration::ZERO,
    )
    .await
    .expect("the test ends");

    assert_eq!(ended.outcome, Some(wire::SignInTestOutcome::Worked));
    assert_eq!(reads.get(), 4);
}

#[tokio::test]
async fn a_refused_read_stops_waiting_rather_than_reading_on() {
    let reads = Cell::new(0);
    let refused = awaited(
        test_in("running", None),
        || {
            reads.set(reads.get() + 1);
            async { Err(anyhow!(Failed::new(Exit::Unresolved, "gone"))) }
        },
        Duration::ZERO,
    )
    .await
    .expect_err("a refusal ends the wait");

    assert_eq!(Exit::of(&refused), Exit::Unresolved);
    assert_eq!(reads.get(), 1);
}

#[test]
fn a_method_naming_providers_needs_one_of_their_models() {
    let opencode = method(Some(&["opencode", "opencode-go"]));

    assert!(model_named(&opencode, Some("opencode-go/glm-5.3")).is_ok());
    for model in [
        None,
        Some("glm-5.3"),
        Some("anthropic/claude"),
        Some("opencode/"),
    ] {
        let refused = model_named(&opencode, model).expect_err("no usable model");
        assert!(refused.contains("--model PROVIDER/MODEL"), "{refused}");
        assert!(refused.contains("opencode-go"), "{refused}");
    }
    assert!(model_named(&method(None), None).is_ok());
}

#[test]
fn an_ended_failure_keeps_its_kind_and_exit_and_names_the_test() {
    for (outcome, exit) in [
        (Some("authentication_failed"), Exit::Rejected),
        (Some("not_covered"), Exit::Rejected),
        (Some("unavailable"), Exit::Unavailable),
        (None, Exit::Unavailable),
    ] {
        let state = if outcome.is_some() {
            "failed"
        } else {
            "interrupted"
        };
        let failed = failed("ada", test_in(state, outcome));
        assert_eq!(failed.exit(), exit);
        let message = diagnostic::message(failed.diagnostic().expect("typed"));
        assert!(message.contains("t1"), "{message}");
        assert!(message.contains("not run again"), "{message}");
    }
}
