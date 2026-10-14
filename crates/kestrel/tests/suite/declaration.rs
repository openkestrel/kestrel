use crate::support::{Kestrel, fixture::Fixture};
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn posted(kestrel: &Kestrel, path: &str, body: &Value) -> (StatusCode, Value) {
    let answer = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .json(body)
        .send()
        .await
        .expect("the operator boundary should answer");
    let status = answer.status();
    let text = answer.text().await.expect("an answer");
    let body = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{text} is not JSON: {error}"))
    };
    (status, body)
}

async fn refused(kestrel: &Kestrel, path: &str, body: &Value) -> Value {
    let (status, refusal) = posted(kestrel, path, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["kind"], "invalid_field", "{refusal}");
    refusal
}

/// The module raised the field itself, the caller stamped its operation and prefixed the
/// route's record onto it, and the correction offered is the caller's operation.
fn names_the_field(refusal: &Value, field: &str, constraint: &str, message: &str, operation: &str) {
    assert_eq!(refusal["field"], field, "{refusal}");
    assert_eq!(refusal["context"]["field"], field, "{refusal}");
    assert_eq!(refusal["context"]["constraint"], constraint, "{refusal}");
    assert_eq!(refusal["message"], message, "{refusal}");
    let next = &refusal["next_steps"][0];
    assert_eq!(next["action"], "correct_field", "{refusal}");
    assert_eq!(next["operation"], operation, "{refusal}");
    assert_eq!(next["field"], field, "{refusal}");
    assert_eq!(next["constraint"], constraint, "{refusal}");
}

fn projects_of(organization: &str) -> String {
    operator::PROJECTS.replace("{organization}", organization)
}

fn agents_of(organization: &str) -> String {
    operator::AGENTS.replace("{organization}", organization)
}

fn triggers_of(organization: &str) -> String {
    operator::TRIGGERS.replace("{organization}", organization)
}

fn trigger_test_at(organization: &str, trigger: &str) -> String {
    operator::TRIGGER_TEST
        .replace("{organization}", organization)
        .replace("{trigger}", trigger)
}

fn declaration_of(organization: &str) -> String {
    operator::DECLARATION.replace("{organization}", organization)
}

fn declaration_preview_of(organization: &str) -> String {
    operator::DECLARATION_PREVIEW.replace("{organization}", organization)
}

fn applied_triggers_of(organization: &str) -> String {
    operator::APPLIED_TRIGGERS.replace("{organization}", organization)
}

fn applied_triggers_preview_of(organization: &str) -> String {
    operator::APPLIED_TRIGGERS_PREVIEW.replace("{organization}", organization)
}

/// Every Trigger declaration's rendering half, with the schedule form spliced in.
fn trigger_declaration(form: Value) -> Value {
    let mut declaration = json!({
        "name": "sweep",
        "brief": "Sweep the backlog",
        "project": "kestrel",
        "agent": "builder",
    });
    for (field, value) in form.as_object().expect("a schedule form is an object") {
        declaration
            .as_object_mut()
            .expect("a declaration is an object")
            .insert(field.clone(), value.clone());
    }
    declaration
}

/// The one Trigger parser refuses the form that is not a schedule, on the field that is
/// missing or the one sitting beside the schedule that is there.
#[tokio::test]
async fn a_schedule_form_is_refused_on_the_field_that_is_not_one() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let only_one = "a trigger declares a filter, an interval or a cron expression, and only one";

    for (form, field, message) in [
        (
            json!({
                "filter": { "exact": { "type": "example" } },
                "every": "1h",
            }),
            "filter",
            only_one,
        ),
        (
            json!({
                "filter": { "exact": { "type": "example" } },
                "cron": "0 9 * * *",
                "zone": "UTC",
            }),
            "filter",
            only_one,
        ),
        (
            json!({ "cron": "0 9 * * *" }),
            "zone",
            "a cron expression declares the time zone it is read in",
        ),
        (
            json!({ "every": "1h", "zone": "UTC" }),
            "zone",
            "a time zone is declared only with a cron expression",
        ),
    ] {
        let refusal = refused(
            &kestrel,
            &triggers_of("acme"),
            &trigger_declaration(form.clone()),
        )
        .await;
        names_the_field(&refusal, field, "well_formed", message, "declare_trigger");
    }

    kestrel.teardown().await;
}

/// A firing that would exhaust its budget is refused on the schedule field, wherever a
/// schedule arrives.
#[tokio::test]
async fn a_firing_faster_than_its_budget_is_refused_on_its_schedule_field() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let impatient = refused(
        &kestrel,
        &triggers_of("acme"),
        &trigger_declaration(json!({ "every": "1m" })),
    )
    .await;
    names_the_field(
        &impatient,
        "every",
        "well_formed",
        "a trigger firing every 1m would exhaust its budget of 10 firings in 1h: fire at most \
         every 6m",
        "declare_trigger",
    );

    let impatient = refused(
        &kestrel,
        &triggers_of("acme"),
        &trigger_declaration(json!({ "cron": "* * * * *", "zone": "UTC" })),
    )
    .await;
    names_the_field(
        &impatient,
        "cron",
        "well_formed",
        "a trigger firing as often as every 1m would exhaust its budget of 10 firings in 1h: \
         fire at most every 6m",
        "declare_trigger",
    );

    kestrel.teardown().await;
}

/// A schedule that is not readable is refused on the field that is not, with the reason the
/// calendar gave.
#[tokio::test]
async fn a_malformed_schedule_is_refused_on_its_field_with_its_reason() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    for (form, field, message) in [
        (
            json!({ "cron": "0 9 * * *", "zone": "Mars/Olympus_Mons" }),
            "cron",
            "a trigger schedule: ",
        ),
        (
            json!({ "every": "whenever" }),
            "every",
            "a trigger schedule is a duration: ",
        ),
    ] {
        let refusal = refused(
            &kestrel,
            &triggers_of("acme"),
            &trigger_declaration(form.clone()),
        )
        .await;
        assert_eq!(refusal["field"], field, "{form}: {refusal}");
        assert_eq!(
            refusal["context"]["constraint"], "well_formed",
            "{form}: {refusal}"
        );
        assert!(
            refusal["message"]
                .as_str()
                .expect("a message")
                .starts_with(message),
            "{form}: {refusal}"
        );
        assert_eq!(
            refusal["next_steps"][0]["operation"], "declare_trigger",
            "{form}: {refusal}"
        );
    }

    kestrel.teardown().await;
}

/// A filter or template field is read wherever a trigger is parsed, and the refusal names the
/// field under the record it declared, stamped with the caller's operation.
#[tokio::test]
async fn a_document_trigger_refuses_its_parsed_field_under_its_record() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let document = json!({
        "project": { "name": "kestrel", "repositories": ["https://github.com/jtmthf/kestrel"], "branch": "main" },
        "agent": { "name": "builder", "harness": "opencode" },
        "trigger": {
            "name": "sweep",
            "filter": { "exact": { "type": "example" } },
            "brief": "Sweep the backlog",
            "project": "kestrel",
            "agent": "builder",
        },
    });

    for (bad, field, message) in [
        (
            json!({ "filter": { "bogus": { "type": "example" } } }),
            "trigger.filter",
            "a trigger filter: bogus is not a filter kestrel knows: use exact, prefix, suffix, \
             all, any or not",
        ),
        (
            json!({ "on_miss": "nonsense" }),
            "trigger.on_miss",
            "a trigger on_miss: nonsense is not what a trigger does when correlation misses",
        ),
        (
            json!({ "on_open_workspace": "restart" }),
            "trigger.on_open_workspace",
            "a trigger on_open_workspace: restart is not what a trigger does to an open \
             workspace",
        ),
    ] {
        let declared = spliced(&document, &["trigger"], bad);
        let refusal = refused(&kestrel, &declaration_preview_of("acme"), &declared).await;
        names_the_field(
            &refusal,
            field,
            "well_formed",
            message,
            "preview_declaration",
        );
    }

    let declared = spliced(
        &document,
        &["trigger"],
        json!({ "correlation": "{{ event" }),
    );
    let refusal = refused(&kestrel, &declaration_preview_of("acme"), &declared).await;
    assert_eq!(refusal["field"], "trigger.correlation", "{refusal}");
    assert_eq!(refusal["context"]["constraint"], "well_formed", "{refusal}");
    assert!(
        refusal["message"]
            .as_str()
            .expect("a message")
            .starts_with("a trigger correlation: "),
        "{refusal}"
    );
    assert_eq!(
        refusal["next_steps"][0]["operation"], "preview_declaration",
        "{refusal}"
    );

    kestrel.teardown().await;
}

/// A correlation is declared together with what a miss does with it, in the one place it is
/// read.
#[tokio::test]
async fn a_trigger_must_declare_its_miss_where_it_declares_its_correlation() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let declared = json!({
        "project": { "name": "kestrel", "repositories": ["https://github.com/jtmthf/kestrel"], "branch": "main" },
        "agent": { "name": "builder", "harness": "opencode" },
        "trigger": {
            "name": "sweep",
            "filter": { "exact": { "type": "example" } },
            "brief": "Sweep the backlog",
            "correlation": "{{ event.source }}{{ event.subject }}",
            "project": "kestrel",
            "agent": "builder",
        },
    });

    let refusal = refused(&kestrel, &declaration_preview_of("acme"), &declared).await;
    names_the_field(
        &refusal,
        "trigger.correlation",
        "well_formed",
        "a trigger with a correlation must declare what it does when it misses",
        "preview_declaration",
    );

    kestrel.teardown().await;
}

/// An empty name is refused by the same sentence on every path, the field named under the
/// record the path declares it for.
#[tokio::test]
async fn a_declaration_names_each_record_wherever_a_name_is_empty() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let message = "a declaration names each record";

    for (body, path, operation) in [
        (
            json!({
                "name": "",
                "repositories": ["https://github.com/jtmthf/kestrel"],
                "branch": "main",
            }),
            projects_of("acme"),
            "declare_project",
        ),
        (
            json!({ "name": "", "harness": "opencode" }),
            agents_of("acme"),
            "declare_agent",
        ),
        (
            json!({
                "name": "",
                "filter": { "exact": { "type": "example" } },
                "brief": "Sweep the backlog",
                "project": "kestrel",
                "agent": "builder",
            }),
            triggers_of("acme"),
            "declare_trigger",
        ),
    ] {
        let refusal = refused(&kestrel, &path, &body).await;
        assert_eq!(
            (),
            names_the_field(&refusal, "name", "non_empty", message, operation),
            "{path}"
        );
    }
    assert!(list(&kestrel, &projects_of("acme")).await.is_empty());
    assert!(list(&kestrel, &agents_of("acme")).await.is_empty());
    assert!(list(&kestrel, &triggers_of("acme")).await.is_empty());

    for (what, field) in [
        ("project", "project.name"),
        ("agent", "agent.name"),
        ("trigger", "trigger.name"),
    ] {
        let mut document = json!({
            "project": { "name": "kestrel", "repositories": ["https://github.com/jtmthf/kestrel"], "branch": "main" },
            "agent": { "name": "builder", "harness": "opencode" },
            "trigger": {
                "name": "sweep",
                "filter": { "exact": { "type": "example" } },
                "brief": "Sweep the backlog",
                "project": "kestrel",
                "agent": "builder",
            },
        });
        document[what]["name"] = json!("");
        let refusal = refused(&kestrel, &declaration_of("acme"), &document).await;
        names_the_field(&refusal, field, "non_empty", message, "apply_declaration");
    }

    kestrel.teardown().await;
}

/// A start refuses what would change a declaration on the first field that would differ, in
/// the sentence it has always said, now typed by the module.
#[tokio::test]
async fn a_start_refuses_its_declaration_change_on_the_first_differing_field() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let start = json!({
        "organization": "acme",
        "project": {
            "name": "kestrel",
            "repositories": ["https://github.com/jtmthf/kestrel"],
            "branch": "next",
        },
        "agent": { "name": "builder", "harness": "opencode", "model": null },
        "brief": "Fix the flaky test",
    });

    let (status, refusal) = posted(&kestrel, operator::STARTS, &start).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["kind"], "invalid_field", "{refusal}");
    names_the_field(
        &refusal,
        "project.branch",
        "immutable",
        "the project kestrel is declared against https://github.com/jtmthf/kestrel on main, and \
         a start changes no declaration",
        "start",
    );
    assert!(
        list(
            &kestrel,
            &operator::WORKSPACES.replace("{organization}", "acme")
        )
        .await
        .is_empty(),
        "a refused start opened a workspace"
    );

    kestrel.teardown().await;
}

/// A document Trigger names the Project and Agent that document declares, so a Trigger can
/// never point at a record its own document does not write.
#[tokio::test]
async fn a_document_trigger_names_the_declared_project_and_agent() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let declared = |project: &str, agent: &str| {
        json!({
            "project": { "name": "kestrel", "repositories": ["https://github.com/jtmthf/kestrel"], "branch": "main" },
            "agent": { "name": "builder", "harness": "opencode" },
            "trigger": {
                "name": "sweep",
                "filter": { "exact": { "type": "example" } },
                "brief": "Sweep the backlog",
                "project": project,
                "agent": agent,
            },
        })
    };

    let refusal = refused(
        &kestrel,
        &declaration_of("acme"),
        &declared("elsewhere", "builder"),
    )
    .await;
    names_the_field(
        &refusal,
        "trigger.project",
        "matches_declared",
        "the trigger names project elsewhere, not the declared project kestrel",
        "apply_declaration",
    );

    let refusal = refused(
        &kestrel,
        &declaration_of("acme"),
        &declared("kestrel", "fixer"),
    )
    .await;
    names_the_field(
        &refusal,
        "trigger.agent",
        "matches_declared",
        "the trigger names agent fixer, not the declared agent builder",
        "apply_declaration",
    );

    kestrel.teardown().await;
}

/// The applied-Triggers file checks each entry through the same module: the sentence is the one
/// every other path answers, and the field is under the entry it was read from.
#[tokio::test]
async fn an_applied_trigger_refusal_names_the_entry_in_its_field_path() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let entry = |name: &str, fields: Value| {
        let mut entry = json!({
            "filter": { "exact": { "type": "example" } },
            "brief": "Sweep the backlog",
            "project": "kestrel",
            "agent": "builder",
        });
        for (field, value) in fields.as_object().expect("an entry is an object") {
            entry[field] = value.clone();
        }
        json!({ "triggers": { name: entry } })
    };

    for (file, field, constraint, message) in [
        (
            entry(
                "nightly",
                json!({ "filter": { "bogus": { "type": "example" } } }),
            ),
            "triggers.nightly.filter",
            "well_formed",
            "a trigger filter: bogus is not a filter kestrel knows: use exact, prefix, suffix, \
             all, any or not",
        ),
        (
            entry("sweep", json!({ "on_open_workspace": "restart" })),
            "triggers.sweep.on_open_workspace",
            "well_formed",
            "a trigger on_open_workspace: restart is not what a trigger does to an open workspace",
        ),
        (
            entry("", json!({})),
            "triggers..name",
            "non_empty",
            "a declaration names each record",
        ),
    ] {
        for (path, operation) in [
            (applied_triggers_of("acme"), "apply_triggers"),
            (
                applied_triggers_preview_of("acme"),
                "preview_applied_triggers",
            ),
        ] {
            let refusal = refused(&kestrel, &path, &file).await;
            names_the_field(&refusal, field, constraint, message, operation);
        }
    }
    assert!(list(&kestrel, &triggers_of("acme")).await.is_empty());

    kestrel.teardown().await;
}

/// The document and the file take a filter alone and close their schemas, which the boundary
/// refuses for the generated types that read past both; an applied Trigger answers its kind.
#[tokio::test]
async fn a_document_and_a_file_refuse_a_schedule_and_a_field_they_do_not_have() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let document = json!({
        "project": { "name": "kestrel", "repositories": ["acme/api"], "branch": "main" },
        "agent": { "name": "builder", "harness": "opencode" },
        "trigger": {
            "name": "sweep",
            "filter": { "exact": { "type": "example" } },
            "brief": "Sweep the backlog",
            "project": "kestrel",
            "agent": "builder",
        },
    });
    let file = json!({ "triggers": { "sweep": {
        "filter": { "exact": { "type": "example" } },
        "brief": "Sweep the backlog",
        "project": "kestrel",
        "agent": "builder",
    } } });

    for (path, body, field, said) in [
        (
            declaration_preview_of("acme"),
            spliced(&document, &["trigger"], json!({ "every": "1h" })),
            json!("trigger.every"),
            "a declaration document's trigger fires on a filter, not a schedule",
        ),
        (
            declaration_of("acme"),
            spliced(&document, &[], json!({ "workspace": {} })),
            Value::Null,
            "unknown field `workspace`",
        ),
        (
            declaration_of("acme"),
            spliced(&document, &["agent"], json!({ "modle": "claude-opus-5" })),
            Value::Null,
            "unknown field `agent.modle`",
        ),
        (
            applied_triggers_of("acme"),
            spliced(&file, &["triggers", "sweep"], json!({ "every": "1h" })),
            Value::Null,
            "triggers.sweep: unknown field `every`",
        ),
        (
            trigger_test_at("acme", "sweep"),
            json!({ "declared": spliced(&file, &["triggers", "sweep"], json!({ "every": "1h" })) }),
            Value::Null,
            "triggers.sweep: unknown field `every`",
        ),
    ] {
        let (status, refusal) = posted(&kestrel, &path, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
        assert_eq!(refusal["kind"], "malformed_request", "{refusal}");
        assert_eq!(refusal["field"], field, "{refusal}");
        assert_eq!(refusal["message"], said, "{refusal}");
    }
    assert!(list(&kestrel, &triggers_of("acme")).await.is_empty());

    let (status, applied) = posted(&kestrel, &applied_triggers_of("acme"), &file).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["changes"][0]["kind"], "trigger", "{applied}");
    assert_eq!(applied["changes"][0]["name"], "sweep", "{applied}");
    assert_eq!(applied["changes"][0]["action"], "add", "{applied}");

    kestrel.teardown().await;
}

/// The same broken rule answers the same constraint and sentence whichever path carried it;
/// only the record its field is under differs.
#[tokio::test]
async fn one_broken_rule_answers_the_same_refusal_on_every_path() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let message = "a trigger on_open_workspace: restart is not what a trigger does to an open \
                   workspace";
    let trigger = json!({
        "name": "sweep",
        "filter": { "exact": { "type": "example" } },
        "brief": "Sweep the backlog",
        "on_open_workspace": "restart",
        "project": "kestrel",
        "agent": "builder",
    });
    let mut entry = trigger.clone();
    entry.as_object_mut().expect("an object").remove("name");

    for (path, body, field, operation) in [
        (
            triggers_of("acme"),
            trigger.clone(),
            "on_open_workspace",
            "declare_trigger",
        ),
        (
            declaration_of("acme"),
            json!({
                "project": {
                    "name": "kestrel",
                    "repositories": ["https://example.com/acme/kestrel.git"],
                    "branch": "main",
                },
                "agent": { "name": "builder", "harness": "scripted" },
                "trigger": trigger,
            }),
            "trigger.on_open_workspace",
            "apply_declaration",
        ),
        (
            applied_triggers_of("acme"),
            json!({ "triggers": { "sweep": entry } }),
            "triggers.sweep.on_open_workspace",
            "apply_triggers",
        ),
    ] {
        let refusal = refused(&kestrel, &path, &body).await;
        names_the_field(&refusal, field, "well_formed", message, operation);
    }

    kestrel.teardown().await;
}

/// A Trigger is tested as a file declares it only when that file declares the Trigger being
/// tested.
#[tokio::test]
async fn a_trigger_is_tested_against_a_declaration_the_file_must_name() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let file = json!({ "declared": { "triggers": { "other": {
        "filter": { "exact": { "type": "example" } },
        "brief": "Tidy up",
        "project": "kestrel",
        "agent": "builder",
    } } } });

    let (status, refusal) = posted(&kestrel, &trigger_test_at("acme", "ready"), &file).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");
    assert_eq!(refusal["kind"], "missing_reference", "{refusal}");
    assert_eq!(
        refusal["message"], "the declaration file declares no trigger ready",
        "{refusal}"
    );
    assert_eq!(refusal["context"]["resource"], "trigger", "{refusal}");
    assert_eq!(refusal["context"]["reference"], "ready", "{refusal}");
    assert_eq!(refusal["context"]["organization"], "acme", "{refusal}");

    kestrel.teardown().await;
}

/// The module's one change vocabulary, seen whole through the boundary: whatever the route,
/// every record a declaration reaches answers its kind, name, action and differences.
#[tokio::test]
async fn every_record_a_declaration_reaches_answers_what_the_write_made() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let document = |branch: &str| {
        json!({
            "project": { "name": "kestrel", "repositories": ["acme/api"], "branch": branch },
            "agent": { "name": "builder", "harness": "opencode", "model": "claude-opus-5" },
            "trigger": {
                "name": "ready",
                "filter": { "exact": { "type": "example" } },
                "brief": "Work on {{ event.type }}",
                "project": "kestrel",
                "agent": "builder",
            },
        })
    };

    let (_, applied) = posted(&kestrel, &declaration_of("acme"), &document("main")).await;
    for (kind, name) in [
        ("project", "kestrel"),
        ("agent", "builder"),
        ("trigger", "ready"),
    ] {
        let declared = applied["declarations"]
            .as_array()
            .expect("the declarations")
            .iter()
            .find(|declared| declared["kind"] == kind)
            .unwrap_or_else(|| panic!("{kind} is not in {applied}"));
        assert_eq!(declared["action"], "add", "{applied}");
        assert_eq!(declared["name"], name, "{applied}");
        assert!(
            !declared["differences"]
                .as_array()
                .expect("an add names every field")
                .is_empty(),
            "{applied}"
        );
    }

    let (_, previewed) = posted(&kestrel, &declaration_preview_of("acme"), &document("main")).await;
    for declared in previewed["declarations"]
        .as_array()
        .expect("the declarations")
    {
        assert_eq!(declared["action"], "unchanged", "{previewed}");
    }

    let (_, previewed) = posted(&kestrel, &declaration_preview_of("acme"), &document("next")).await;
    assert_eq!(
        previewed["declarations"][0]["action"], "change",
        "{previewed}"
    );
    assert_eq!(
        previewed["declarations"][1]["action"], "unchanged",
        "{previewed}"
    );
    assert_eq!(
        previewed["declarations"][2]["action"], "unchanged",
        "{previewed}"
    );
    let branch = previewed["declarations"][0]["differences"]
        .as_array()
        .expect("the project's differences")
        .iter()
        .find(|difference| difference["field"] == "branch")
        .expect("the branch would differ");
    assert_eq!(branch["was"], "main", "{previewed}");
    assert_eq!(branch["becomes"], "next", "{previewed}");
    assert_eq!(list(&kestrel, &projects_of("acme")).await.len(), 1);
    assert_eq!(
        list(
            &kestrel,
            &operator::WORKSPACES.replace("{organization}", "acme")
        )
        .await
        .len(),
        0,
        "a preview applied its declaration"
    );

    kestrel.teardown().await;
}

fn spliced(declaration: &Value, path: &[&str], field: Value) -> Value {
    let mut spliced = declaration.clone();
    let mut at = &mut spliced;
    for step in path {
        at = &mut at[*step];
    }
    let at = at.as_object_mut().expect("a record is an object");
    for (field, value) in field.as_object().expect("a field is an object") {
        at.insert(field.clone(), value.clone());
    }
    spliced
}

async fn list(kestrel: &Kestrel, path: &str) -> Vec<Value> {
    let listed = reqwest::get(format!("{}{path}", kestrel.operator()))
        .await
        .expect("the operator boundary should answer");
    assert_eq!(listed.status(), StatusCode::OK);
    let body: Value = listed.json().await.expect("an answer");
    body.as_array().expect("an array of records").clone()
}
