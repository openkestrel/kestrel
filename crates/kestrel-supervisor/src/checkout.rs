use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::link::{Checkout, Git, Observed};

/// Into the working directory the agent is then spawned in. A branch the remote does not have
/// yet is cut from the base, and a checkout an earlier Session on this Instance left is left as it
/// is, with whatever it holds that the remote does not.
pub async fn check_out(checkout: &Checkout) -> Result<(), String> {
    let Checkout {
        repositories,
        base,
        branch,
    } = checkout;

    for repository in repositories {
        let directory = cloned_into(repository);
        if Path::new(directory).join(".git").exists() {
            continue;
        }
        let failed = |why: String| {
            format!("{repository} could not be checked out on the branch {branch}: {why}")
        };

        git(&["clone", "--branch", base, repository, directory])
            .await
            .map_err(failed)?;
        if branch != base
            && git(&["-C", directory, "checkout", branch, "--"])
                .await
                .is_err()
            && let Err(why) = git(&["-C", directory, "checkout", "-b", branch]).await
        {
            // Or the next Session on this Instance would take a clone on the base for its branch.
            let _ = std::fs::remove_dir_all(directory);
            return Err(failed(why));
        }
    }

    Ok(())
}

pub async fn observe(checkout: &Checkout) -> Vec<Observed> {
    let mut observed = Vec::with_capacity(checkout.repositories.len());

    for repository in &checkout.repositories {
        let git = match read(Path::new(cloned_into(repository))).await {
            Ok(read) => read,
            Err(because) => Git::Unreadable { because },
        };
        observed.push(Observed {
            repository: repository.clone(),
            git,
        });
    }

    observed
}

/// Unpushed counts every commit no remote-tracking branch reaches, on any local branch or a
/// detached HEAD, because the agent is free to leave work on a branch kestrel never declared.
async fn read(directory: &Path) -> Result<Git, String> {
    if !directory.join(".git").exists() {
        return Err(format!("there is no checkout at {}", directory.display()));
    }
    let directory = &*directory.to_string_lossy();

    let status = git(&[
        "-C",
        directory,
        "status",
        "--porcelain",
        "--untracked-files=all",
    ])
    .await?;
    let untracked = status.lines().filter(|line| line.starts_with("??")).count();
    let uncommitted = status.lines().count() - untracked;
    let stashes = git(&["-C", directory, "stash", "list"])
        .await?
        .lines()
        .count();
    let unpushed = git(&[
        "-C",
        directory,
        "rev-list",
        "--count",
        "HEAD",
        "--branches",
        "--not",
        "--remotes",
    ])
    .await?;
    let branch = git(&["-C", directory, "branch", "--show-current"]).await?;

    Ok(Git::Read {
        branch: (!branch.is_empty()).then_some(branch),
        untracked: untracked as u64,
        uncommitted: uncommitted as u64,
        stashes: stashes as u64,
        unpushed: unpushed
            .parse()
            .map_err(|_| format!("git counted {unpushed:?} unpushed commits"))?,
    })
}

pub async fn work(checkout: &Checkout) -> Vec<crate::link::WorkRepository> {
    let mut repositories = Vec::with_capacity(checkout.repositories.len());
    for repository in &checkout.repositories {
        let name = cloned_into(repository);
        let git = match tokio::time::timeout(
            Duration::from_secs(10),
            work_in(Path::new(name), &checkout.branch),
        )
        .await
        {
            Ok(Ok(git)) => git,
            Ok(Err(because)) => crate::link::WorkGit::Unreadable { because },
            Err(_) => crate::link::WorkGit::Unreadable {
                because: "git did not report the work within 10 seconds".to_owned(),
            },
        };
        repositories.push(crate::link::WorkRepository {
            repository: name.to_owned(),
            git,
        });
    }
    repositories
}

async fn work_in(directory: &Path, declared: &str) -> Result<crate::link::WorkGit, String> {
    use crate::link::{Commits, WorkGit};
    let directory = &*directory.to_string_lossy();
    let branch = git(&["-C", directory, "branch", "--show-current"]).await?;
    let changed = numstat(
        &git(&[
            "-C",
            directory,
            "diff",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--find-renames",
            "--no-textconv",
        ])
        .await?,
    )?;
    let staged = numstat(
        &git(&[
            "-C",
            directory,
            "diff",
            "--cached",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--find-renames",
            "--no-textconv",
        ])
        .await?,
    )?;
    let commits = git(&[
        "-C",
        directory,
        "rev-list",
        "--count",
        "HEAD",
        "--branches",
        "--not",
        "--remotes",
    ])
    .await?;
    let committed = numstat(
        &git(&[
            "-C",
            directory,
            "log",
            "--format=",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--find-renames",
            "--no-textconv",
            "--diff-merges=first-parent",
            "HEAD",
            "--branches",
            "--not",
            "--remotes",
        ])
        .await?,
    )?;
    let pushed = git(&[
        "-C",
        directory,
        "rev-parse",
        "--verify",
        &format!("refs/remotes/origin/{declared}"),
    ])
    .await
    .ok();
    let untracked = git(&[
        "-C",
        directory,
        "ls-files",
        "--others",
        "--exclude-standard",
        "-z",
    ])
    .await?
    .split('\0')
    .filter(|path| !path.is_empty())
    .count() as u64;
    let stashed = git(&["-C", directory, "stash", "list", "--format=%H"])
        .await?
        .lines()
        .count() as u64;
    Ok(WorkGit::Read {
        branch: (!branch.is_empty()).then_some(branch),
        changed,
        staged,
        committed: Commits {
            commits: commits
                .parse()
                .map_err(|_| format!("git counted {commits:?} commits"))?,
            added: committed.added,
            removed: committed.removed,
        },
        pushed,
        untracked,
        stashed,
    })
}

fn numstat(output: &str) -> Result<crate::link::Changes, String> {
    let mut stats = crate::link::Changes {
        files: 0,
        added: 0,
        removed: 0,
    };
    let mut paths = std::collections::HashSet::new();
    let mut records = output.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        let mut fields = record.trim_start_matches('\n').splitn(3, '\t');
        let added = fields.next().unwrap_or_default();
        let removed = fields
            .next()
            .ok_or_else(|| format!("git returned an invalid numstat: {record:?}"))?;
        let mut path = fields
            .next()
            .ok_or_else(|| format!("git returned an invalid numstat: {record:?}"))?;
        if path.is_empty() {
            records
                .next()
                .ok_or_else(|| "git omitted a renamed file's old path".to_owned())?;
            path = records
                .next()
                .ok_or_else(|| "git omitted a renamed file's new path".to_owned())?;
        }
        paths.insert(path);
        for (number, count) in [(added, &mut stats.added), (removed, &mut stats.removed)] {
            if number != "-" {
                *count += number
                    .parse::<u64>()
                    .map_err(|_| format!("git returned an invalid line count: {number:?}"))?;
            }
        }
    }
    stats.files = paths.len() as u64;
    Ok(stats)
}

pub fn root(checkout: Option<&Checkout>) -> PathBuf {
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    match checkout.and_then(|checkout| checkout.repositories.first()) {
        Some(first) => here.join(cloned_into(first)),
        None => here,
    }
}

/// The directory `git clone` would choose for itself, named so the checkout after it can find it.
pub(crate) fn cloned_into(repository: &str) -> &str {
    let name = repository
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repository);

    name.strip_suffix(".git").unwrap_or(name)
}

const GIT_GIVES_UP_AFTER: Duration = Duration::from_secs(10 * 60);

async fn git(arguments: &[&str]) -> Result<String, String> {
    Ok(untrimmed(arguments).await?.trim().to_owned())
}

/// For `-z` output, where a name may start or end with whitespace.
pub(crate) async fn untrimmed(arguments: &[&str]) -> Result<String, String> {
    let running = Command::new("git")
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .kill_on_drop(true)
        .output();
    let ran = tokio::time::timeout(GIT_GIVES_UP_AFTER, running)
        .await
        .map_err(|_| format!("git did not finish within {GIT_GIVES_UP_AFTER:?}"))?
        .map_err(|error| format!("git could not be run: {error}"))?;

    if !ran.status.success() {
        return Err(String::from_utf8_lossy(&ran.stderr).trim().to_owned());
    }

    Ok(String::from_utf8_lossy(&ran.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Stdio;

    use tempfile::TempDir;

    use super::*;

    /// A clone of a remote holding one commit, on a branch of its own the remote has never seen.
    struct Cloned {
        _held: TempDir,
        checkout: PathBuf,
    }

    impl Cloned {
        fn new() -> Self {
            let held = TempDir::new().expect("a temporary directory");
            let remote = held.path().join("remote");
            let checkout = held.path().join("checkout");
            std::fs::create_dir(&remote).expect("the remote should be made");
            run(&remote, &["init", "--initial-branch", "main"]);
            std::fs::write(remote.join("README.md"), "published\n").expect("a file");
            run(&remote, &["add", "README.md"]);
            commit(&remote, "published");
            run(
                held.path(),
                &[
                    "clone",
                    &remote.to_string_lossy(),
                    &checkout.to_string_lossy(),
                ],
            );
            run(&checkout, &["checkout", "-b", "kestrel/work"]);

            Self {
                _held: held,
                checkout,
            }
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.checkout.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, contents).expect("the file should be written");
        }

        fn git(&self, arguments: &[&str]) {
            run(&self.checkout, arguments);
        }

        async fn read(&self) -> Git {
            read(&self.checkout)
                .await
                .expect("the checkout should read")
        }
    }

    fn commit(directory: &Path, message: &str) {
        run(directory, &["commit", "--all", "--message", message]);
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

    fn only(untracked: u64, uncommitted: u64, stashes: u64, unpushed: u64) -> Git {
        Git::Read {
            branch: Some("kestrel/work".to_owned()),
            untracked,
            uncommitted,
            stashes,
            unpushed,
        }
    }

    #[test]
    fn numstat_counts_binary_files_and_paths_containing_tabs_or_newlines() {
        let stats = numstat("2\t1\tsource.rs\0-\t-\timage.png\0\n3\t0\ta\tb\nc\0").unwrap();
        assert_eq!(
            stats,
            crate::link::Changes {
                files: 3,
                added: 5,
                removed: 1
            }
        );
    }

    #[test]
    fn numstat_counts_a_rename_as_one_file() {
        let stats = numstat("1\t2\t\0old name\0new name\0").unwrap();
        assert_eq!(
            stats,
            crate::link::Changes {
                files: 1,
                added: 1,
                removed: 2
            }
        );
    }

    #[tokio::test]
    async fn a_merge_conflict_counts_as_one_changed_file() {
        let cloned = Cloned::new();
        cloned.write("README.md", "declared branch\n");
        commit(&cloned.checkout, "declared branch");
        cloned.git(&["checkout", "-b", "side", "main"]);
        cloned.write("README.md", "side branch\n");
        commit(&cloned.checkout, "side branch");
        cloned.git(&["checkout", "kestrel/work"]);
        cloned.git(&["config", "user.name", "kestrel"]);
        cloned.git(&["config", "user.email", "kestrel@example.com"]);
        assert!(
            git(&["-C", &cloned.checkout.to_string_lossy(), "merge", "side"])
                .await
                .is_err()
        );
        let summary = work_in(&cloned.checkout, "kestrel/work").await.unwrap();
        let crate::link::WorkGit::Read { changed, .. } = summary else {
            panic!("unreadable");
        };
        assert_eq!(changed.files, 1);
    }

    #[tokio::test]
    async fn a_fresh_branch_cut_from_the_remote_holds_nothing_of_its_own() {
        assert_eq!(Cloned::new().read().await, only(0, 0, 0, 0));
    }

    #[tokio::test]
    async fn every_untracked_file_is_counted_however_deep() {
        let cloned = Cloned::new();
        cloned.write("notes.md", "a note");
        cloned.write("research/findings/one.md", "a finding");

        assert_eq!(cloned.read().await, only(2, 0, 0, 0));
    }

    #[tokio::test]
    async fn an_edit_left_uncommitted_is_counted() {
        let cloned = Cloned::new();
        cloned.write("README.md", "edited\n");

        assert_eq!(cloned.read().await, only(0, 1, 0, 0));
    }

    #[tokio::test]
    async fn a_stash_is_counted() {
        let cloned = Cloned::new();
        cloned.write("README.md", "edited\n");
        cloned.git(&["stash"]);

        assert_eq!(cloned.read().await, only(0, 0, 1, 0));
    }

    #[tokio::test]
    async fn a_commit_no_remote_branch_reaches_is_unpushed_on_whatever_branch_it_is() {
        let cloned = Cloned::new();
        cloned.write("README.md", "committed\n");
        commit(&cloned.checkout, "on the declared branch");
        cloned.git(&["checkout", "-b", "elsewhere", "main"]);
        cloned.write("README.md", "elsewhere\n");
        commit(&cloned.checkout, "on a branch nobody declared");
        cloned.git(&["checkout", "kestrel/work"]);

        assert_eq!(cloned.read().await, only(0, 0, 0, 2));
    }

    #[tokio::test]
    async fn a_pushed_commit_is_not_unpushed() {
        let cloned = Cloned::new();
        cloned.write("README.md", "committed\n");
        commit(&cloned.checkout, "pushed");
        cloned.git(&["push", "origin", "kestrel/work"]);

        assert_eq!(cloned.read().await, only(0, 0, 0, 0));
    }

    #[tokio::test]
    async fn ignored_build_output_is_not_counted() {
        let cloned = Cloned::new();
        cloned.write(".git/info/exclude", "target/\n");
        cloned.write("target/debug/built", "output");

        assert_eq!(cloned.read().await, only(0, 0, 0, 0));
    }

    #[tokio::test]
    async fn a_directory_that_is_not_a_checkout_is_unreadable() {
        let held = TempDir::new().expect("a temporary directory");

        let because = read(held.path()).await.expect_err("nothing to read");

        assert!(because.contains("no checkout"), "unhelpful: {because}");
    }

    #[tokio::test]
    async fn a_checkout_git_cannot_read_is_unreadable() {
        let cloned = Cloned::new();
        std::fs::write(cloned.checkout.join(".git/HEAD"), "garbage").expect("HEAD overwritten");

        read(&cloned.checkout)
            .await
            .expect_err("a checkout with a corrupt HEAD should not read");
    }

    #[test]
    fn a_repository_is_cloned_into_the_directory_git_names_for_it() {
        assert_eq!(
            cloned_into("https://github.com/acme/widgets.git"),
            "widgets"
        );
        assert_eq!(cloned_into("https://github.com/acme/widgets/"), "widgets");
        assert_eq!(cloned_into("file:///tmp/kestrel"), "kestrel");
    }
}
