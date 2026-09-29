//! What the suites leave on the daemon, and the sweep that takes it back once the checkout that
//! built it is gone. Every image, volume, network and container a suite creates carries the path
//! of the checkout it belongs to, and the sweep drops the ones whose path is no longer there.
//!
//! The sweep runs by itself at the start of any suite that uses the daemon, so one live checkout
//! collects every checkout deleted since the last run. The test that touches the daemon is
//! ignored by default and CI runs it with `--ignored`.

mod support;

use std::path::Path;

use support::docker;

#[test]
fn a_checkout_names_its_resources_with_a_path_a_sweep_can_still_find() {
    let live = docker::checkout_path(&docker::repository());

    assert!(
        Path::new(&live).is_dir(),
        "a suite would label its resources {live}, which is not a checkout"
    );
    assert!(
        docker::stale(vec![("live", live)]).is_empty(),
        "a sweep took a checkout that is still there"
    );
}

#[test]
fn a_sweep_takes_only_the_namespaces_whose_checkout_is_gone() {
    let live = docker::checkout_path(&docker::repository());
    let gone = a_checkout_that_is_gone();

    let taken = docker::stale(vec![("live", live), ("gone", gone)]);

    assert_eq!(taken, vec!["gone"]);
}

#[test]
#[ignore = "creates and removes docker resources"]
fn the_sweep_takes_what_a_deleted_checkout_left_and_leaves_a_live_one() {
    let gone = a_checkout_that_is_gone();
    let live = docker::checkout_path(&docker::repository());
    let named = |what: &str| format!("kestrel-sweep-{what}-{}", std::process::id());

    let (image, live_image) = (named("image"), named("image-live"));
    let (container, live_container) = (named("container"), named("container-live"));
    let (volume, live_volume) = (named("volume"), named("volume-live"));
    let (network, live_network) = (named("network"), named("network-live"));

    an_image(&image, &gone);
    an_image(&live_image, &live);
    a_container(&container, &image, &gone);
    a_container(&live_container, &live_image, &live);
    labelled(&["volume", "create"], &volume, &gone);
    labelled(&["volume", "create"], &live_volume, &live);
    labelled(&["network", "create"], &network, &gone);
    labelled(&["network", "create"], &live_network, &live);

    docker::sweep_deleted_checkouts();

    for (kind, name) in [
        ("container", &container),
        ("image", &image),
        ("volume", &volume),
        ("network", &network),
    ] {
        assert!(
            !present(&[kind, "inspect", name.as_str()]),
            "the {kind} a deleted checkout left outlived the sweep"
        );
    }
    for (kind, name) in [
        ("container", &live_container),
        ("image", &live_image),
        ("volume", &live_volume),
        ("network", &live_network),
    ] {
        assert!(
            present(&[kind, "inspect", name.as_str()]),
            "the sweep took a live checkout's {kind}"
        );
    }

    docker::completed(
        &["container", "rm", "--force", live_container.as_str()],
        "removing a live container",
    );
    docker::completed(
        &["image", "rm", "--force", live_image.as_str()],
        "removing a live image",
    );
    docker::completed(
        &["volume", "rm", "--force", live_volume.as_str()],
        "removing a live volume",
    );
    docker::completed(
        &["network", "rm", live_network.as_str()],
        "removing a live network",
    );
}

/// A resource the daemon takes a label on directly, and that pins nothing.
fn labelled(create: &[&str], name: &str, checkout: &str) {
    let label = label(checkout);
    let mut command = create.to_vec();
    command.extend_from_slice(&["--label", label.as_str(), name]);

    docker::completed(&command, "creating a labelled resource");
}

/// The only image a test needs: empty, so a build is instant and reaches no registry.
fn an_image(name: &str, checkout: &str) {
    let context = tempfile::TempDir::new().expect("a temporary build context");
    std::fs::write(context.path().join("Dockerfile"), "FROM scratch\n")
        .expect("a Dockerfile to build");
    let label = label(checkout);
    let context = context.path().to_str().expect("a path docker can build");

    docker::completed(
        &["build", "--label", label.as_str(), "--tag", name, context],
        "building a labelled image",
    );
}

fn a_container(name: &str, image: &str, checkout: &str) {
    let label = label(checkout);

    docker::completed(
        // The image is empty, so the command is only what `docker create` insists on.
        &[
            "create",
            "--label",
            label.as_str(),
            "--name",
            name,
            image,
            "true",
        ],
        "creating a labelled container",
    );
}

fn label(checkout: &str) -> String {
    format!("{}={checkout}", docker::CHECKOUT_LABEL)
}

fn present(arguments: &[&str]) -> bool {
    docker::ran(arguments).code == 0
}

/// A path shaped like a checkout that is not on disk, so a sweep takes what names it.
fn a_checkout_that_is_gone() -> String {
    docker::checkout_path(
        &std::env::temp_dir().join(format!("kestrel-gone-{}", std::process::id())),
    )
}
