//! The `kestrel-dev` image: everything an agent working on Kestrel itself needs, and nothing
//! it would have to sign in with.
//!
//! Every test here builds and runs the image, which a `cargo test` has no business doing on
//! its own, so they are ignored by default. CI runs all but `kestrel_passes_its_own_checks`,
//! which compiles the Cargo workspace three times over and runs on a schedule instead.

mod support;

use serde_json::Value;
use support::docker;
use support::image;

#[test]
#[ignore = "builds and runs the kestrel-dev image"]
fn the_harness_label_is_the_product_image_it_derives_from() {
    assert_eq!(
        image::declared_harnesses(image::development()),
        image::declared_harnesses(image::built()),
    );
    assert_eq!(
        image::declared_harnesses(image::development()),
        image::HARNESSES
    );
}

#[test]
#[ignore = "builds and runs the kestrel-dev image"]
fn the_toolchain_git_and_gh_are_each_invocable_in_the_image() {
    let rustc = running(&["rustc", "--version"]);
    assert!(
        rustc.out.starts_with("rustc 1.96.0"),
        "rustc in the image is not the toolchain rust-toolchain.toml names: {rustc:?}"
    );

    for command in [
        &["cargo", "--version"][..],
        &["cargo", "fmt", "--version"],
        &["cargo", "clippy", "--version"],
        &["git", "--version"],
        &["gh", "--version"],
    ] {
        let ran = running(command);
        assert_eq!(ran.code, 0, "{command:?} in the image said {ran:?}");
    }
}

#[test]
#[ignore = "builds and runs the kestrel-dev image"]
fn each_harness_answers_an_acp_handshake_in_the_image() {
    for harness in image::HARNESS_COMMANDS {
        let answer = image::handshake(image::development(), harness);
        assert_eq!(
            answer["result"]["protocolVersion"], 1,
            "{harness:?} answered initialize with {answer}"
        );
    }
}

#[test]
#[ignore = "builds and runs the kestrel-dev image"]
fn the_image_carries_no_credentials() {
    let variables = docker::configured(image::development(), "{{json .Config.Env}}");
    let names: Vec<Value> = serde_json::from_str(&variables).expect("the image's environment");
    for name in names.iter().filter_map(Value::as_str) {
        let name = name.split('=').next().unwrap_or_default().to_uppercase();
        assert!(
            !["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"]
                .iter()
                .any(|word| name.contains(word)),
            "the image sets {name}"
        );
    }

    let home = running(&[
        "find",
        "/home/kestrel",
        "-mindepth",
        "1",
        "!",
        "-name",
        ".bashrc",
        "!",
        "-name",
        ".profile",
        "!",
        "-name",
        ".bash_logout",
    ]);
    assert_eq!(home.code, 0, "sweeping the home directory said {home:?}");
    assert!(
        home.out.is_empty(),
        "a login would be found in what the image puts in its home directory:\n{}",
        home.out
    );
}

#[test]
#[ignore = "builds kestrel-dev and compiles the Cargo workspace in it"]
fn kestrel_passes_its_own_checks_in_the_image() {
    let checkout = format!("{}:/workspace/kestrel:ro", docker::repository().display());
    let ran = docker::ran(&[
        "run",
        "--rm",
        "--volume",
        &checkout,
        "--workdir",
        "/workspace/kestrel",
        "--env",
        "CARGO_TARGET_DIR=/tmp/target",
        "--entrypoint",
        "sh",
        image::development(),
        "-c",
        "set -e
         cargo fmt --all --check
         cargo clippy --locked --workspace --all-targets -- -D warnings
         cargo build --locked --workspace
         cargo test --locked --workspace",
    ]);

    assert_eq!(
        ran.code, 0,
        "kestrel failed its own checks in the image:\n{}\n{}",
        ran.out, ran.err
    );
}

fn running(command: &[&str]) -> docker::Ran {
    docker::running(image::development(), command)
}
