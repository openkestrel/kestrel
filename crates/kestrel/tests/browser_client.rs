//! The browser Client's built assets, served on the operator listener beside the operator
//! interface (ADR-0036).

mod support;

use std::fs;

use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use support::Kestrel;
use tempfile::TempDir;

const SHELL: &str = "<!doctype html><title>kestrel</title>";

fn built_client() -> TempDir {
    let built = TempDir::new().expect("a directory for the built Client");
    fs::write(built.path().join("index.html"), SHELL).expect("the shell");
    fs::create_dir(built.path().join("assets")).expect("the assets directory");
    fs::write(built.path().join("assets/main-abc123.js"), "export {}").expect("an asset");
    built
}

async fn get(kestrel: &Kestrel, path: &str) -> reqwest::Response {
    reqwest::get(format!("{}{path}", kestrel.operator()))
        .await
        .unwrap_or_else(|error| panic!("GET {path} should be answered: {error}"))
}

#[tokio::test]
async fn a_deep_link_is_answered_with_the_shell_every_time_it_is_loaded() {
    let built = built_client();
    let kestrel = Kestrel::boot_serving_client(built.path()).await;

    for _ in 0..2 {
        let page = get(
            &kestrel,
            "/organizations/acme/workspaces/brave-otter-abcdefgh",
        )
        .await;

        assert_eq!(page.status(), StatusCode::OK);
        assert!(
            page.headers()[CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html"),
            "the shell is served as {:?}",
            page.headers()[CONTENT_TYPE]
        );
        assert_eq!(page.text().await.unwrap(), SHELL);
    }
    assert_eq!(get(&kestrel, "/").await.text().await.unwrap(), SHELL);
}

#[tokio::test]
async fn a_deep_link_naming_something_with_a_dot_is_still_the_shell() {
    let built = built_client();
    let kestrel = Kestrel::boot_serving_client(built.path()).await;

    for path in ["/organizations/acme.io", "/organizations/acme.io/new"] {
        let page = get(&kestrel, path).await;

        assert_eq!(page.status(), StatusCode::OK, "{path}");
        assert_eq!(page.text().await.unwrap(), SHELL, "{path}");
    }
}

#[tokio::test]
async fn a_built_asset_is_served_as_itself() {
    let built = built_client();
    let kestrel = Kestrel::boot_serving_client(built.path()).await;

    let asset = get(&kestrel, "/assets/main-abc123.js").await;

    assert_eq!(asset.status(), StatusCode::OK);
    assert!(
        asset.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("javascript"),
        "the asset is served as {:?}",
        asset.headers()[CONTENT_TYPE]
    );
    assert_eq!(asset.text().await.unwrap(), "export {}");
}

#[tokio::test]
async fn a_missing_asset_is_not_found_rather_than_the_shell() {
    let built = built_client();
    let kestrel = Kestrel::boot_serving_client(built.path()).await;

    for path in [
        "/assets/main-gone.js",
        "/favicon.ico",
        "/assets/%2e%2e/%2e%2e/Cargo.toml",
    ] {
        let missing = get(&kestrel, path).await;

        assert_eq!(missing.status(), StatusCode::NOT_FOUND, "{path}");
        assert_ne!(missing.text().await.unwrap(), SHELL, "{path}");
    }
}

#[tokio::test]
async fn an_unknown_operator_path_is_refused_by_the_operator_interface_not_the_shell() {
    let built = built_client();
    let kestrel = Kestrel::boot_serving_client(built.path()).await;

    for path in [
        "/operator",
        "/operator/nothing-here",
        "/operator/organizations/acme/nope",
    ] {
        let refused = get(&kestrel, path).await;

        assert_eq!(refused.status(), StatusCode::NOT_FOUND, "{path}");
        let body: serde_json::Value = refused
            .json()
            .await
            .unwrap_or_else(|error| panic!("{path} answered no refusal: {error}"));
        assert!(body["message"].is_string(), "{path} answered {body}");
    }
}

#[tokio::test]
async fn a_client_route_is_not_found_on_a_control_plane_serving_no_client() {
    let kestrel = Kestrel::boot().await;

    let page = get(&kestrel, "/organizations/acme").await;

    assert_eq!(page.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_reads_reach_the_client() {
    let built = built_client();
    let kestrel = Kestrel::boot_serving_client(built.path()).await;

    let posted = reqwest::Client::new()
        .post(format!("{}/organizations/acme", kestrel.operator()))
        .send()
        .await
        .expect("a POST should be answered");

    assert_eq!(posted.status(), StatusCode::METHOD_NOT_ALLOWED);
}
