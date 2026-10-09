use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const HELPERS: [(&str, &str); 3] = [
    ("kestrel-supervisor", "kestrel-supervisor"),
    ("kestrel-scripted-agent", "kestrel-scripted-agent"),
    ("kestrel-client", "kestrel"),
];

struct Prepared {
    directory: PathBuf,
    artifacts: BTreeMap<String, Artifact>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Artifact {
    size: u64,
    modified: SystemTime,
    inode: u64,
    mode: u32,
}

impl Artifact {
    fn at(path: &Path) -> Self {
        let metadata = fs::metadata(path).unwrap_or_else(|error| {
            panic!("executable helper is missing: {}: {error}", path.display())
        });
        assert!(
            metadata.is_file() && metadata.mode() & 0o111 != 0,
            "executable helper is not executable: {}",
            path.display()
        );
        Self {
            size: metadata.len(),
            modified: metadata.modified().unwrap_or_else(|error| {
                panic!(
                    "executable helper {} has no modification time: {error}",
                    path.display()
                )
            }),
            inode: metadata.ino(),
            mode: metadata.mode(),
        }
    }
}

pub fn binary(package: &str) -> PathBuf {
    named(package, package)
}

pub fn named(package: &str, binary: &str) -> PathBuf {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    assert!(
        HELPERS.contains(&(package, binary)),
        "unknown helper {package}/{binary}"
    );
    let prepared = PREPARED.get_or_init(|| {
        let directory = alongside_this_test();
        let artifacts = prepare(&directory);
        Prepared {
            directory,
            artifacts,
        }
    });
    let artifact = prepared.directory.join(binary);
    assert_eq!(
        Artifact::at(&artifact),
        prepared.artifacts[binary],
        "executable helper is stale: {}",
        artifact.display()
    );
    artifact
}

fn prepare(alongside: &Path) -> BTreeMap<String, Artifact> {
    let lock_path = alongside.join(".kestrel-helpers.lock");
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap_or_else(|error| panic!("{} could not open: {error}", lock_path.display()));
    lock.lock()
        .unwrap_or_else(|error| panic!("{} could not lock: {error}", lock_path.display()));
    let identity = format!("{}\n{}", super::crate_root().display(), invocation());
    let identity: String = Sha256::digest(identity)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let stamp = alongside.join(format!(".kestrel-helpers-{identity}.stamp"));
    match fs::read(&stamp) {
        Ok(contents) => {
            let artifacts: BTreeMap<String, Artifact> = serde_json::from_slice(&contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "invalid helper preparation stamp {}: {error}",
                        stamp.display()
                    )
                });
            for (_, binary) in HELPERS {
                let artifact = alongside.join(binary);
                let expected = artifacts.get(binary).unwrap_or_else(|| {
                    panic!(
                        "{} has no prepared artifact {}",
                        stamp.display(),
                        artifact.display()
                    )
                });
                assert_eq!(
                    Artifact::at(&artifact),
                    *expected,
                    "executable helper is stale: {}",
                    artifact.display()
                );
            }
            return artifacts;
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => panic!("{} could not read: {error}", stamp.display()),
    }
    let profile = alongside
        .file_name()
        .and_then(|profile| profile.to_str())
        .expect("a named profile directory");
    let target = alongside
        .parent()
        .expect("the profile sits in the target directory");
    let mut command = Command::new(env!("CARGO"));
    command.current_dir(super::crate_root()).args([
        "build",
        "--bins",
        "--profile",
        if profile == "debug" { "dev" } else { profile },
    ]);
    command.arg("--target-dir").arg(target);
    for (package, _) in HELPERS {
        command.args(["--package", package]);
    }
    eprintln!(
        "Preparing executable test helpers in {}",
        alongside.display()
    );
    let built = command.output().unwrap_or_else(|error| {
        panic!(
            "Cargo could not prepare helpers in {}: {error}",
            alongside.display()
        )
    });
    assert!(
        built.status.success(),
        "building helpers {:?} in {} failed:\n{}",
        HELPERS,
        alongside.display(),
        String::from_utf8_lossy(&built.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&built.stderr));
    let artifacts: BTreeMap<_, _> = HELPERS
        .into_iter()
        .map(|(_, binary)| {
            let artifact = alongside.join(binary);
            detach_from_cargo_cache(&artifact);
            (binary.to_owned(), Artifact::at(&artifact))
        })
        .collect();
    fs::write(
        &stamp,
        serde_json::to_vec(&artifacts).expect("helper metadata should serialize"),
    )
    .unwrap_or_else(|error| panic!("{} could not write: {error}", stamp.display()));
    for entry in fs::read_dir(alongside)
        .unwrap_or_else(|error| panic!("{} could not read: {error}", alongside.display()))
    {
        let entry = entry.unwrap_or_else(|error| {
            panic!("{} could not read entry: {error}", alongside.display())
        });
        let path = entry.path();
        let name = entry.file_name();
        if path != stamp
            && name.to_str().is_some_and(|name| {
                name.starts_with(".kestrel-helpers-") && name.ends_with(".stamp")
            })
        {
            fs::remove_file(&path)
                .unwrap_or_else(|error| panic!("{} could not remove: {error}", path.display()));
        }
    }
    artifacts
}

fn detach_from_cargo_cache(artifact: &Path) {
    let metadata = fs::metadata(artifact).unwrap_or_else(|error| {
        panic!(
            "executable helper is missing: {}: {error}",
            artifact.display()
        )
    });
    if metadata.nlink() <= 1 {
        return;
    }
    let detached = artifact.with_extension("kestrel-helper");
    fs::copy(artifact, &detached).unwrap_or_else(|error| {
        panic!(
            "executable helper {} could not detach from Cargo's cache: {error}",
            artifact.display()
        )
    });
    fs::rename(&detached, artifact).unwrap_or_else(|error| {
        panic!(
            "executable helper {} could not publish: {error}",
            artifact.display()
        )
    });
}

fn invocation() -> String {
    if let Ok(run) = std::env::var("NEXTEST_RUN_ID") {
        assert!(
            !run.is_empty(),
            "NEXTEST_RUN_ID must identify an invocation"
        );
        return format!("nextest:{run}");
    }
    let parent = std::os::unix::process::parent_id();
    let started = Command::new("ps")
        .args(["-p", &parent.to_string(), "-o", "lstart="])
        .output()
        .expect("the Cargo parent's start time should be readable");
    assert!(
        started.status.success(),
        "could not identify Cargo parent {parent}"
    );
    let started = String::from_utf8(started.stdout).expect("ps should report a UTF-8 start time");
    assert!(
        !started.trim().is_empty(),
        "Cargo parent {parent} has no start time"
    );
    format!("cargo:{parent}:{}", started.trim())
}

fn alongside_this_test() -> PathBuf {
    std::env::current_exe()
        .expect("a test binary should know where it is")
        .parent()
        .and_then(Path::parent)
        .expect("the test binary sits in the profile's deps directory")
        .to_path_buf()
}
