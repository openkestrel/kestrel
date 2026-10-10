//! What the configured image declares it carries, read from its metadata through the Docker
//! daemon, and the declarations and starts that read gates (ADR-0048).
//!
//! Every test here builds images on the host daemon, which a `cargo test` has no business doing
//! on its own, so they are ignored by default and CI runs them with `--ignored`.

mod support;

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::docker;

/// Only metadata: nothing here is ever run, so an image with no filesystem is enough.
struct Labelled {
    reference: String,
}

impl Labelled {
    fn declaring(harnesses: &str) -> Self {
        Self::built(&format!(
            "FROM scratch\nLABEL dev.kestrel.harnesses=\"{harnesses}\"\n"
        ))
    }

    fn built(dockerfile: &str) -> Self {
        let reference = unique("built");
        Self::build(&reference, dockerfile);
        Self { reference }
    }

    /// A tag that is moved, so `reference` names whatever was built last.
    fn build(reference: &str, dockerfile: &str) {
        let mut building = Command::new("docker")
            .args(["build", "--quiet", "--tag", reference, "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("docker should build");
        building
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(dockerfile.as_bytes())
            .expect("docker should read the Dockerfile");
        let built = building.wait_with_output().expect("docker should finish");
        assert!(
            built.status.success(),
            "building {reference} failed: {}",
            String::from_utf8_lossy(&built.stderr)
        );
    }

    fn identity(&self) -> String {
        docker::completed(
            &["image", "inspect", "--format", "{{.Id}}", &self.reference],
            "reading the image's identity",
        )
    }

    fn tagged(&self, reference: &str) -> Self {
        docker::completed(
            &["tag", &self.reference, reference],
            "tagging the image again",
        );
        Self {
            reference: reference.to_owned(),
        }
    }
}

impl Drop for Labelled {
    fn drop(&mut self) {
        let _ = docker::ran(&["image", "rm", "--force", &self.reference]);
    }
}

fn unique(what: &str) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    format!(
        "kestrel-capability-{}-{}:{what}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

async fn provisioning_from(image: &str) -> Kestrel {
    Kestrel::dispatching_harnesses_in(
        image,
        &[
            ("opencode", "opencode acp"),
            ("claude", "claude-agent-acp"),
            ("codex", "codex-acp"),
            ("my-agent", "/opt/my-agent --acp"),
        ],
    )
    .await
}

async fn availability(kestrel: &Kestrel) -> Vec<(String, Value)> {
    let response = reqwest::get(format!("{}/operator/harnesses", kestrel.operator()))
        .await
        .expect("the operator boundary answers");
    assert_eq!(response.status(), StatusCode::OK);
    let rows: Value = response.json().await.expect("catalogue rows");
    let _: Vec<kestrel_operator_types::HarnessCatalogueEntry> =
        serde_json::from_value(rows.clone()).expect("the authored catalogue shape");
    rows.as_array()
        .expect("catalogue rows")
        .iter()
        .map(|row| {
            (
                row["name"].as_str().expect("a name").to_owned(),
                row["availability"].clone(),
            )
        })
        .collect()
}

async fn states(kestrel: &Kestrel) -> Vec<(String, String)> {
    availability(kestrel)
        .await
        .into_iter()
        .map(|(name, availability)| {
            (
                name,
                availability["state"].as_str().expect("a state").to_owned(),
            )
        })
        .collect()
}

fn named(states: &[(&str, &str)]) -> Vec<(String, String)> {
    states
        .iter()
        .map(|(name, state)| ((*name).to_owned(), (*state).to_owned()))
        .collect()
}

async fn posted(kestrel: &Kestrel, path: &str, body: Value) -> (StatusCode, Value) {
    let response = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .json(&body)
        .send()
        .await
        .expect("the operator boundary answers");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn read(kestrel: &Kestrel, path: &str) -> Value {
    reqwest::get(format!("{}{path}", kestrel.operator()))
        .await
        .expect("the operator boundary answers")
        .json()
        .await
        .expect("a JSON read")
}

async fn declared_organization(kestrel: &Kestrel) {
    let (status, _) = posted(
        kestrel,
        "/operator/organizations",
        json!({ "name": "acme" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

fn document(harness: &str) -> Value {
    json!({
        "project": {
            "name": "site",
            "repositories": ["https://github.com/acme/site"],
            "branch": "main",
        },
        "agent": { "name": "coder", "harness": harness },
        "trigger": {
            "name": "on-issue",
            "filter": { "exact": { "type": "com.github.issues.opened" } },
            "brief": "Fix it",
            "project": "site",
            "agent": "coder",
        },
    })
}

fn plan(organization: &str, harness: &str) -> Value {
    json!({
        "organization": organization,
        "project": {
            "name": "site",
            "repositories": ["https://github.com/acme/site"],
            "branch": "main",
        },
        "agent": { "name": "coder", "harness": harness },
        "brief": "Fix it",
    })
}

fn refused_as_not_carried(refusal: &Value, image: &str, harness: &str) {
    let _: kestrel_operator_types::Diagnostic =
        serde_json::from_value(refusal.clone()).expect("the authored diagnostic shape");
    assert_eq!(refusal["kind"], "setup_gap", "{refusal}");
    assert_eq!(refusal["context"]["prerequisite"], "harness_in_image");
    assert_eq!(refusal["context"]["image"], image);
    assert_eq!(refusal["context"]["harness"], harness);
    assert_eq!(
        refusal["next_steps"][0],
        json!({
            "action": "inspect_harness_image",
            "harness": harness,
            "image": image,
            "command": null,
        })
    );
    assert_eq!(refusal["next_steps"][1]["action"], "correct_field");
}

fn refused_as_unavailable(refusal: &Value, image: &str) {
    let _: kestrel_operator_types::Diagnostic =
        serde_json::from_value(refusal.clone()).expect("the authored diagnostic shape");
    assert_eq!(refusal["kind"], "unavailable", "{refusal}");
    assert_eq!(refusal["context"]["service"], "image_inspection");
    assert_eq!(refusal["context"]["resource"], image);
    let steps: Vec<_> = refusal["next_steps"]
        .as_array()
        .expect("next steps")
        .iter()
        .map(|step| step["action"].as_str().expect("an action").to_owned())
        .collect();
    assert_eq!(steps, ["inspect_harness_image", "retry_read"]);
}

#[tokio::test]
#[ignore = "builds images on the host daemon"]
async fn declaring_an_agent_on_a_harness_the_image_omits_is_refused_naming_both() {
    let image = Labelled::declaring("opencode,my-agent");
    let kestrel = provisioning_from(&image.reference).await;
    declared_organization(&kestrel).await;

    let (status, refusal) = posted(
        &kestrel,
        "/operator/organizations/acme/agents",
        json!({ "name": "coder", "harness": "claude" }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    refused_as_not_carried(&refusal, &image.reference, "claude");
    assert_eq!(refusal["field"], "harness");
    assert_eq!(
        refusal["next_steps"][1]["allowed_values"],
        json!(["my-agent", "opencode"])
    );
    assert_eq!(
        read(&kestrel, "/operator/organizations/acme/agents").await,
        json!([])
    );

    // A name outside the catalogue that the image declares is evidence enough to accept.
    for harness in ["opencode", "my-agent"] {
        let (status, agent) = posted(
            &kestrel,
            "/operator/organizations/acme/agents",
            json!({ "name": harness, "harness": harness }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{agent}");
    }
}

#[tokio::test]
#[ignore = "builds images on the host daemon"]
async fn a_refused_declaration_preview_apply_or_start_leaves_nothing_behind() {
    let image = Labelled::declaring("opencode");
    let kestrel = provisioning_from(&image.reference).await;
    declared_organization(&kestrel).await;

    for path in [
        "/operator/organizations/acme/declaration/preview",
        "/operator/organizations/acme/declaration",
    ] {
        let (status, refusal) = posted(&kestrel, path, document("codex")).await;
        assert_eq!(status, StatusCode::CONFLICT, "{path}");
        refused_as_not_carried(&refusal, &image.reference, "codex");
        assert_eq!(refusal["field"], "agent.harness");
    }
    let (status, refusal) = posted(&kestrel, "/operator/starts", plan("globex", "codex")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    refused_as_not_carried(&refusal, &image.reference, "codex");

    for (path, nothing) in [
        ("/operator/organizations/acme/projects", json!([])),
        ("/operator/organizations/acme/agents", json!([])),
        ("/operator/organizations/acme/triggers", json!([])),
    ] {
        assert_eq!(read(&kestrel, path).await, nothing, "{path}");
    }
    let organizations = read(&kestrel, "/operator/organizations").await;
    assert_eq!(
        organizations.as_array().map(Vec::len),
        Some(1),
        "{organizations}"
    );

    let (status, applied) = posted(
        &kestrel,
        "/operator/organizations/acme/declaration",
        document("opencode"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
}

#[tokio::test]
#[ignore = "builds images on the host daemon"]
async fn availability_follows_the_label_of_whatever_image_the_tag_names_now() {
    let every = Labelled::declaring("opencode,claude,codex,somebody-elses");
    let reference = unique("moving");
    let tag = every.tagged(&reference);
    let kestrel = provisioning_from(&reference).await;

    // Unknown names add no rows: the catalogue alone says what a harness is.
    assert_eq!(
        states(&kestrel).await,
        named(&[
            ("opencode", "available"),
            ("claude", "available"),
            ("codex", "available"),
        ])
    );
    let (_, opencode) = availability(&kestrel).await.remove(0);
    assert_eq!(opencode["image"], reference.as_str());
    assert_eq!(opencode["identity"], every.identity().as_str());

    // A derived image inherits the label, and overrides it to declare fewer.
    Labelled::build(
        &reference,
        &format!(
            "FROM {}\nLABEL dev.kestrel.harnesses=\"opencode\"\n",
            every.reference
        ),
    );

    assert_eq!(
        states(&kestrel).await,
        named(&[
            ("opencode", "available"),
            ("claude", "not_carried"),
            ("codex", "not_carried"),
        ])
    );
    let (_, opencode) = availability(&kestrel).await.remove(0);
    assert_eq!(opencode["identity"], tag.identity().as_str());
    assert_ne!(opencode["identity"], every.identity().as_str());
    declared_organization(&kestrel).await;
    let (status, refusal) = posted(
        &kestrel,
        "/operator/organizations/acme/agents",
        json!({ "name": "coder", "harness": "claude" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    refused_as_not_carried(&refusal, &reference, "claude");
}

#[tokio::test]
#[ignore = "builds images on the host daemon"]
async fn an_image_gone_since_a_positive_read_is_unavailable_and_never_pulled() {
    let image = Labelled::declaring("opencode,claude,codex");
    let kestrel = provisioning_from(&image.reference).await;
    assert!(
        states(&kestrel)
            .await
            .iter()
            .all(|(_, state)| state == "available")
    );

    let reference = image.reference.clone();
    drop(image);

    for (name, availability) in availability(&kestrel).await {
        assert_eq!(availability["state"], "unavailable", "{name}");
        assert_eq!(availability["identity"], Value::Null, "{name}");
        refused_as_unavailable(&availability["diagnostic"], &reference);
    }
    declared_organization(&kestrel).await;
    let (status, refusal) = posted(
        &kestrel,
        "/operator/organizations/acme/agents",
        json!({ "name": "coder", "harness": "opencode" }),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    refused_as_unavailable(&refusal, &reference);
    let (status, refusal) = posted(&kestrel, "/operator/starts", plan("acme", "opencode")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    refused_as_unavailable(&refusal, &reference);

    assert_ne!(
        docker::ran(&["image", "inspect", &reference]).code,
        0,
        "the control plane pulled {reference}"
    );
    assert_eq!(
        read(&kestrel, "/operator/organizations/acme/agents").await,
        json!([])
    );
}

#[tokio::test]
#[ignore = "builds images on the host daemon"]
async fn an_image_named_with_a_registry_path_or_by_its_identity_is_inspected() {
    let image = Labelled::declaring("codex");
    let pathed = image.tagged(&format!(
        "localhost:5000/kestrel-capability/{}",
        unique("pathed").replace(':', "-")
    ));

    for reference in [pathed.reference.clone(), image.identity()] {
        let kestrel = provisioning_from(&reference).await;
        assert_eq!(
            states(&kestrel).await,
            named(&[
                ("opencode", "not_carried"),
                ("claude", "not_carried"),
                ("codex", "available"),
            ]),
            "{reference}"
        );
    }
}
