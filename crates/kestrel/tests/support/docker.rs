//! The docker CLI as a test drives it: one place an invocation goes through, and one place a
//! failure says what it was doing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

pub fn ran(arguments: &[&str]) -> Ran {
    ran_against(&[], arguments)
}

/// `ran` with the given variables in the docker process's environment rather than the test
/// process's own, which is how the compose suite names the resources a checkout owns.
pub fn ran_against(variables: &[(&str, &str)], arguments: &[&str]) -> Ran {
    let mut command = docker();
    for (key, value) in variables {
        command.env(key, value);
    }
    let ran = command
        .args(arguments)
        .output()
        .expect("docker should be reachable");

    Ran {
        code: ran.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&ran.stdout).trim().to_owned(),
        err: String::from_utf8_lossy(&ran.stderr).trim().to_owned(),
    }
}

fn docker() -> Command {
    let mut command = Command::new("docker");
    command.current_dir(repository());
    command
}

pub fn completed(arguments: &[&str], doing: &str) -> String {
    let ran = ran(arguments);
    assert_eq!(ran.code, 0, "{doing} failed:\n{}", ran.err);

    ran.out
}

/// Run instead of whatever the image would otherwise start.
pub fn running(image: &str, command: &[&str]) -> Ran {
    let (program, arguments) = command.split_first().expect("a command to run");
    let mut run = vec!["run", "--rm", "--entrypoint", program, image];
    run.extend_from_slice(arguments);

    ran(&run)
}

pub fn configured(image: &str, field: &str) -> String {
    completed(
        &["image", "inspect", "--format", field, image],
        "inspecting the image",
    )
}

pub fn removed(name: &str) {
    let _ = Command::new("docker")
        .args(["rm", "--force", "--volumes", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Every image here builds from the repository root, so that is where a test invokes docker.
/// Read when the test runs, not when it compiled: checkouts sharing a target directory run
/// whichever binary compiled last, and each must still build and tag its own source.
pub fn repository() -> PathBuf {
    super::crate_root()
        .ancestors()
        .nth(2)
        .expect("the crate sits two directories under the repository")
        .to_path_buf()
}

pub fn checkout_digest(checkout: &Path) -> String {
    let canonical = canonical(checkout);
    let mut hashed = Sha256::new();
    hashed.update(canonical.to_string_lossy().as_bytes());

    kestrel::hex::encode(&hashed.finalize())[..16].to_owned()
}

/// The label every suite stamps on what it leaves on the daemon, naming the checkout that owns
/// it, so a sweep can tell a live checkout's resources from a deleted one's.
pub const CHECKOUT_LABEL: &str = "kestrel.test.checkout";

/// The checkout a label names: canonical, so one checkout reached by another path is still one,
/// and a sweep can ask whether it is on disk at all.
pub fn checkout_path(checkout: &Path) -> String {
    canonical(checkout).to_string_lossy().into_owned()
}

fn canonical(checkout: &Path) -> PathBuf {
    std::fs::canonicalize(checkout).unwrap_or_else(|_| checkout.to_path_buf())
}

/// The resources a sweep takes: those whose label names a checkout that is no longer on disk.
/// The checkout running the sweep keeps its own, because its path is still there.
pub fn stale<T>(resources: impl IntoIterator<Item = (T, String)>) -> Vec<T> {
    resources
        .into_iter()
        .filter(|(_, checkout)| !Path::new(checkout).exists())
        .map(|(resource, _)| resource)
        .collect()
}

/// Takes back what every checkout deleted since the last run left on the daemon. A suite runs
/// this before it builds, so one live checkout collects every other that is gone. Containers
/// go first: an image or a volume a live container holds is one the daemon will not take.
pub fn sweep_deleted_checkouts() {
    for name in stale(labelled_containers()) {
        ran(&["container", "rm", "--force", name.as_str()]);
    }
    for name in stale(labelled_images()) {
        ran(&["image", "rm", "--force", name.as_str()]);
    }
    for name in stale(labelled_volumes()) {
        ran(&["volume", "rm", "--force", name.as_str()]);
    }
    for name in stale(labelled_networks()) {
        ran(&["network", "rm", name.as_str()]);
    }
}

/// `(name, checkout)` for every resource of one kind carrying the label. The value is read back
/// with an inspect rather than taken from the filter, so a resource the filter matched without
/// carrying the label itself is left alone.
fn labelled(list: &[&str], inspect: &[&str]) -> Vec<(String, String)> {
    let filter = format!("label={CHECKOUT_LABEL}");
    let mut listing = list.to_vec();
    listing.extend_from_slice(&["--filter", &filter]);
    let listed = ran(&listing).out;
    let mut names = listed.lines().map(str::to_owned).collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return Vec::new();
    }

    let mut inspecting = inspect.to_vec();
    inspecting.extend(names.iter().map(String::as_str));

    ran(&inspecting)
        .out
        .lines()
        .filter_map(|line| {
            let (name, labels) = line.split_once('\t')?;
            let labels: HashMap<String, String> = serde_json::from_str(labels).ok()?;
            let checkout = labels.get(CHECKOUT_LABEL)?;

            Some((name.to_owned(), checkout.clone()))
        })
        .collect()
}

fn labelled_images() -> Vec<(String, String)> {
    labelled(
        &["image", "ls", "--quiet", "--no-trunc"],
        &[
            "image",
            "inspect",
            "--format",
            "{{.Id}}\t{{json .Config.Labels}}",
        ],
    )
}

fn labelled_containers() -> Vec<(String, String)> {
    labelled(
        &["container", "ls", "--all", "--quiet", "--no-trunc"],
        &[
            "container",
            "inspect",
            "--format",
            "{{.Id}}\t{{json .Config.Labels}}",
        ],
    )
}

fn labelled_volumes() -> Vec<(String, String)> {
    labelled(
        &["volume", "ls", "--quiet"],
        &[
            "volume",
            "inspect",
            "--format",
            "{{.Name}}\t{{json .Labels}}",
        ],
    )
}

fn labelled_networks() -> Vec<(String, String)> {
    labelled(
        &["network", "ls", "--quiet"],
        &[
            "network",
            "inspect",
            "--format",
            "{{.Name}}\t{{json .Labels}}",
        ],
    )
}
