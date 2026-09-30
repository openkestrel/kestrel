use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use tokio::io::AsyncReadExt as _;

use crate::checkout;
use crate::link::{Answer, Answered, Checkout, Entry, EntryKind, Tracking};

pub const LISTED: usize = 5_000;
pub const INLINE: u64 = 1024 * 1024;

pub struct Checkouts {
    pub root: PathBuf,
    pub repositories: Vec<String>,
}

impl Checkouts {
    pub fn of(checkout: Option<&Checkout>) -> Self {
        Self {
            root: checkout::root(None),
            repositories: checkout
                .map(|checkout| {
                    checkout
                        .repositories
                        .iter()
                        .map(|repository| checkout::cloned_into(repository).to_owned())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

struct Resolved {
    repository: PathBuf,
    target: PathBuf,
}

pub async fn list(checkouts: &Checkouts, path: Option<&str>) -> Answer {
    let path = normalized(path.unwrap_or_default());
    if path.is_empty() {
        let entries: Vec<Entry> = checkouts
            .repositories
            .iter()
            .map(|name| Entry {
                name: name.clone(),
                kind: EntryKind::Directory,
                size: None,
                git: None,
            })
            .collect();
        return Answer::Listing {
            path,
            total: entries.len() as u64,
            entries,
            truncated: false,
        };
    }
    let resolved = match resolved(checkouts, &path) {
        Ok(resolved) => resolved,
        Err(refusal) => return refusal,
    };
    if !resolved.target.is_dir() {
        return Answer::Refused {
            message: format!("{path} is not a directory"),
        };
    }

    let mut entries = match entries(&resolved.target).await {
        Ok(entries) => entries,
        Err(error) => {
            return Answer::Refused {
                message: format!("{path} could not be listed: {error}"),
            };
        }
    };
    entries.sort_by(|one, other| one.name.cmp(&other.name));
    let total = entries.len() as u64;
    entries.truncate(LISTED);
    if let Some(tracking) = tracking(&resolved).await {
        for entry in &mut entries {
            entry.git = Some(tracking.of(&entry.name));
        }
    }

    Answer::Listing {
        path,
        truncated: total > entries.len() as u64,
        total,
        entries,
    }
}

pub async fn read(checkouts: &Checkouts, path: &str, raw: bool) -> Answered {
    let path = normalized(path);
    let resolved = match resolved(checkouts, &path) {
        Ok(resolved) => resolved,
        Err(refusal) => return Answered::Json(refusal),
    };
    if !resolved.target.is_file() {
        return Answered::Json(Answer::Refused {
            message: format!("{path} is not a file"),
        });
    }
    let file = match tokio::fs::File::open(&resolved.target).await {
        Ok(file) => file,
        Err(error) => {
            return unreadable(&path, &error);
        }
    };
    if raw {
        return streamed(file);
    }

    let mut held = Vec::new();
    if let Err(error) = file.take(INLINE + 1).read_to_end(&mut held).await {
        return unreadable(&path, &error);
    }
    if held.len() as u64 > INLINE {
        return match tokio::fs::File::open(&resolved.target).await {
            Ok(file) => streamed(file),
            Err(error) => unreadable(&path, &error),
        };
    }
    if held.contains(&0) {
        return Answered::Raw(held.into());
    }
    match String::from_utf8(held) {
        Ok(text) => Answered::Json(Answer::Text { path, text }),
        Err(binary) => Answered::Raw(binary.into_bytes().into()),
    }
}

fn unreadable(path: &str, error: &std::io::Error) -> Answered {
    Answered::Json(Answer::Refused {
        message: format!("{path} could not be read: {error}"),
    })
}

fn streamed(file: tokio::fs::File) -> Answered {
    Answered::Raw(reqwest::Body::wrap_stream(
        tokio_util::io::ReaderStream::new(file),
    ))
}

fn normalized(path: &str) -> String {
    path.trim_matches('/').to_owned()
}

/// Symlinks are resolved before the scope is checked, so a link out of the checkout is refused
/// wherever it points.
fn resolved(checkouts: &Checkouts, path: &str) -> Result<Resolved, Answer> {
    let (name, within) = path.split_once('/').unwrap_or((path, ""));
    if !checkouts.repositories.iter().any(|known| known == name) {
        return Err(Answer::Missing {
            message: format!("this Instance checks out no repository named {name}"),
        });
    }
    let repository = checkouts
        .root
        .join(name)
        .canonicalize()
        .map_err(|_| Answer::Missing {
            message: format!("{name} is not checked out on this Instance"),
        })?;
    let target = repository.join(within).canonicalize().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            Answer::Missing {
                message: format!("{path} does not exist"),
            }
        } else {
            Answer::Refused {
                message: format!("{path} could not be resolved: {error}"),
            }
        }
    })?;
    if !target.starts_with(&repository) {
        return Err(Answer::Refused {
            message: format!("{path} resolves outside the {name} checkout"),
        });
    }

    Ok(Resolved { repository, target })
}

async fn entries(directory: &Path) -> std::io::Result<Vec<Entry>> {
    let mut listed = tokio::fs::read_dir(directory).await?;
    let mut entries = Vec::new();
    while let Some(entry) = listed.next_entry().await? {
        let metadata = entry.metadata().await?;
        let kind = if metadata.is_symlink() {
            EntryKind::Symlink
        } else if metadata.is_dir() {
            EntryKind::Directory
        } else if metadata.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        entries.push(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind,
            size: (kind == EntryKind::File).then_some(metadata.len()),
            git: None,
        });
    }

    Ok(entries)
}

/// Each entry's name, by what git knows beneath it. A directory holding any tracked file is
/// tracked, as `git status` would never call it untracked.
struct Tracked {
    tracked: HashSet<String>,
    untracked: HashSet<String>,
    ignored: HashSet<String>,
    /// Set when git collapsed the listed directory itself, or one above it, into one path.
    whole: Option<Tracking>,
}

impl Tracked {
    fn of(&self, name: &str) -> Tracking {
        if self.tracked.contains(name) {
            Tracking::Tracked
        } else if self.untracked.contains(name) {
            Tracking::Untracked
        } else if self.ignored.contains(name) || name == ".git" {
            Tracking::Ignored
        } else {
            self.whole.unwrap_or(Tracking::Untracked)
        }
    }
}

async fn tracking(resolved: &Resolved) -> Option<Tracked> {
    let within = resolved
        .target
        .strip_prefix(&resolved.repository)
        .ok()?
        .to_string_lossy()
        .into_owned();
    let repository = resolved.repository.to_string_lossy();
    let spec = format!(":(literal){within}");
    let spec = (!within.is_empty()).then_some(spec.as_str());

    let tracked = ls_files(&repository, spec, &[]).await.ok()?;
    let untracked = ls_files(
        &repository,
        spec,
        &["--others", "--exclude-standard", "--directory"],
    )
    .await
    .ok()?;
    let ignored = ls_files(
        &repository,
        spec,
        &["--others", "--ignored", "--exclude-standard", "--directory"],
    )
    .await
    .ok()?;

    let mut whole = None;
    let mut named = |listed: &str, as_: Tracking| {
        let mut names = HashSet::new();
        for path in listed.split('\0').filter(|path| !path.is_empty()) {
            let path = path.trim_end_matches('/');
            let beneath = if within.is_empty() {
                Some(path)
            } else {
                path.strip_prefix(&within)
                    .and_then(|rest| rest.strip_prefix('/'))
            };
            match beneath {
                Some(beneath) => {
                    names.insert(beneath.split('/').next().unwrap_or(beneath).to_owned());
                }
                None if within == path || within.starts_with(&format!("{path}/")) => {
                    whole.get_or_insert(as_);
                }
                None => {}
            }
        }
        names
    };

    Some(Tracked {
        tracked: named(&tracked, Tracking::Tracked),
        untracked: named(&untracked, Tracking::Untracked),
        ignored: named(&ignored, Tracking::Ignored),
        whole,
    })
}

async fn ls_files(repository: &str, spec: Option<&str>, only: &[&str]) -> Result<String, String> {
    let mut arguments = vec!["-C", repository, "ls-files", "-z"];
    arguments.extend_from_slice(only);
    arguments.push("--");
    arguments.extend(spec);

    checkout::untrimmed(&arguments).await
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use tempfile::TempDir;

    use super::*;

    struct Cloned {
        _held: TempDir,
        checkouts: Checkouts,
    }

    impl Cloned {
        /// A checkout named `widgets` beside a secret no read may reach.
        fn new() -> Self {
            let held = TempDir::new().expect("a temporary directory");
            let root = held.path().join("root");
            let checkout = root.join("widgets");
            std::fs::create_dir_all(&checkout).expect("the checkout should be made");
            std::fs::write(held.path().join("secret"), "outside\n").expect("a secret");
            run(&checkout, &["init", "--initial-branch", "main"]);
            std::fs::write(checkout.join(".gitignore"), "target/\n*.log\n").expect("a file");
            std::fs::create_dir_all(checkout.join("src")).expect("a directory");
            std::fs::write(checkout.join("src/lib.rs"), "fn tracked() {}\n").expect("a file");
            run(&checkout, &["add", "."]);
            run(&checkout, &["commit", "--message", "tracked"]);

            Self {
                _held: held,
                checkouts: Checkouts {
                    root,
                    repositories: vec!["widgets".to_owned()],
                },
            }
        }

        fn checkout(&self) -> PathBuf {
            self.checkouts.root.join("widgets")
        }

        fn write(&self, path: &str, contents: &[u8]) {
            let path = self.checkout().join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, contents).expect("the file should be written");
        }
    }

    fn run(directory: &Path, arguments: &[&str]) {
        let ran = std::process::Command::new("git")
            .args(arguments)
            .current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "kestrel")
            .env("GIT_AUTHOR_EMAIL", "kestrel@example.com")
            .env("GIT_COMMITTER_NAME", "kestrel")
            .env("GIT_COMMITTER_EMAIL", "kestrel@example.com")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .expect("git should be reachable");
        assert!(
            ran.status.success(),
            "`git {}` failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&ran.stderr)
        );
    }

    fn marked(answer: &Answer) -> Vec<(String, Option<Tracking>)> {
        let Answer::Listing { entries, .. } = answer else {
            panic!("not a listing: {answer:?}");
        };
        entries
            .iter()
            .map(|entry| (entry.name.clone(), entry.git))
            .collect()
    }

    fn named(name: &str, git: Tracking) -> (String, Option<Tracking>) {
        (name.to_owned(), Some(git))
    }

    #[tokio::test]
    async fn no_path_lists_the_repositories() {
        let cloned = Cloned::new();

        assert_eq!(
            marked(&list(&cloned.checkouts, None).await),
            vec![("widgets".to_owned(), None)]
        );
    }

    #[tokio::test]
    async fn a_listing_marks_tracked_untracked_and_ignored_entries() {
        let cloned = Cloned::new();
        cloned.write("notes.md", b"untracked");
        cloned.write("drafts/idea.md", b"untracked beneath");
        cloned.write("target/built", b"ignored");
        cloned.write("run.log", b"ignored");
        cloned.write("src/new.rs", b"untracked beside tracked");

        assert_eq!(
            marked(&list(&cloned.checkouts, Some("widgets")).await),
            vec![
                named(".git", Tracking::Ignored),
                named(".gitignore", Tracking::Tracked),
                named("drafts", Tracking::Untracked),
                named("notes.md", Tracking::Untracked),
                named("run.log", Tracking::Ignored),
                named("src", Tracking::Tracked),
                named("target", Tracking::Ignored),
            ]
        );
        assert_eq!(
            marked(&list(&cloned.checkouts, Some("widgets/src/")).await),
            vec![
                named("lib.rs", Tracking::Tracked),
                named("new.rs", Tracking::Untracked),
            ]
        );
    }

    #[tokio::test]
    async fn inside_a_directory_git_collapsed_every_entry_takes_its_mark() {
        let cloned = Cloned::new();
        cloned.write("target/debug/built", b"ignored");
        cloned.write("drafts/idea.md", b"untracked");

        assert_eq!(
            marked(&list(&cloned.checkouts, Some("widgets/target")).await),
            vec![named("debug", Tracking::Ignored)]
        );
        assert_eq!(
            marked(&list(&cloned.checkouts, Some("widgets/drafts")).await),
            vec![named("idea.md", Tracking::Untracked)]
        );
    }

    #[tokio::test]
    async fn a_directory_past_the_bound_is_truncated_and_says_how_many_it_holds() {
        let cloned = Cloned::new();
        for index in 0..=LISTED {
            cloned.write(&format!("many/{index:05}"), b"");
        }

        let Answer::Listing {
            entries,
            total,
            truncated,
            ..
        } = list(&cloned.checkouts, Some("widgets/many")).await
        else {
            panic!("not a listing");
        };

        assert_eq!(entries.len(), LISTED);
        assert_eq!(total, LISTED as u64 + 1);
        assert!(truncated);
    }

    #[tokio::test]
    async fn a_path_or_symlink_outside_the_checkout_is_refused() {
        let cloned = Cloned::new();
        std::os::unix::fs::symlink(
            cloned.checkouts.root.join("../secret"),
            cloned.checkout().join("escape"),
        )
        .expect("a symlink");
        std::os::unix::fs::symlink(
            cloned.checkouts.root.join(".."),
            cloned.checkout().join("up"),
        )
        .expect("a symlink");

        for path in [
            "widgets/escape",
            "widgets/../../secret",
            "widgets//etc/hosts",
        ] {
            let Answered::Json(Answer::Refused { message }) =
                read(&cloned.checkouts, path, false).await
            else {
                panic!("{path} was not refused");
            };
            assert!(message.contains("outside"), "{message}");
        }
        assert!(matches!(
            list(&cloned.checkouts, Some("widgets/up")).await,
            Answer::Refused { .. }
        ));
    }

    #[tokio::test]
    async fn a_symlink_that_stays_inside_the_checkout_is_followed() {
        let cloned = Cloned::new();
        std::os::unix::fs::symlink("src/lib.rs", cloned.checkout().join("alias.rs"))
            .expect("a symlink");

        let Answered::Json(Answer::Text { text, .. }) =
            read(&cloned.checkouts, "widgets/alias.rs", false).await
        else {
            panic!("the symlink was not followed");
        };
        assert_eq!(text, "fn tracked() {}\n");
    }

    #[tokio::test]
    async fn an_unknown_repository_or_file_is_missing() {
        let cloned = Cloned::new();

        for path in ["elsewhere/README.md", "widgets/absent.md"] {
            assert!(
                matches!(
                    read(&cloned.checkouts, path, false).await,
                    Answered::Json(Answer::Missing { .. })
                ),
                "{path}"
            );
        }
    }

    #[tokio::test]
    async fn text_is_inline_while_binary_large_or_raw_reads_are_streamed() {
        let cloned = Cloned::new();
        cloned.write("binary.bin", &[0, 159, 146, 150]);
        cloned.write("large.txt", &vec![b'a'; INLINE as usize + 1]);

        assert!(matches!(
            read(&cloned.checkouts, "widgets/src/lib.rs", false).await,
            Answered::Json(Answer::Text { .. })
        ));
        for (path, raw) in [
            ("widgets/binary.bin", false),
            ("widgets/large.txt", false),
            ("widgets/src/lib.rs", true),
        ] {
            assert!(
                matches!(read(&cloned.checkouts, path, raw).await, Answered::Raw(_)),
                "{path} was not streamed"
            );
        }
    }
}
