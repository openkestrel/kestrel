use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::checkout;
use crate::files::Checkouts;
use crate::link::{Answer, Checkout, FileStat, Read, RepositoryDiff, RepositoryText};

const DIFF_LIMIT: usize = 2 * 1024 * 1024;

pub async fn read(checkouts: &Checkouts, checkout: Option<&Checkout>, read: Read) -> Answer {
    let reading = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        read_in(checkouts, checkout, &read),
    )
    .await;
    match reading.unwrap_or_else(|_| Err("git did not answer within 10 seconds".to_owned())) {
        Ok(answer) => answer,
        Err(message) => Answer::Refused { message },
    }
}

async fn read_in(
    checkouts: &Checkouts,
    checkout: Option<&Checkout>,
    read: &Read,
) -> Result<Answer, String> {
    let mut diffs = Vec::new();
    let mut remaining = DIFF_LIMIT;
    let mut texts = Vec::new();
    for name in &checkouts.repositories {
        let directory = checkouts.root.join(name);
        let directory = directory.to_string_lossy();
        match read {
            Read::Changes { scope, paths } => {
                let mut filters = Vec::new();
                for path in paths {
                    let (repository, within) = path.split_once('/').unwrap_or((path, ""));
                    if !checkouts.repositories.iter().any(|name| name == repository) {
                        return Err(format!(
                            "this Instance checks out no repository named {repository}"
                        ));
                    }
                    if within.split('/').any(|part| part == ".." || part == ".git")
                        || within.starts_with('/')
                    {
                        return Err(format!("{path} is outside the checkout's files"));
                    }
                    if repository == name {
                        if let Ok(target) = checkouts.root.join(name).join(within).canonicalize()
                            && !target.starts_with(
                                checkouts
                                    .root
                                    .join(name)
                                    .canonicalize()
                                    .map_err(|error| error.to_string())?,
                            )
                        {
                            return Err(format!("{path} resolves outside the {name} checkout"));
                        }
                        let mut normalized = within
                            .split('/')
                            .filter(|part| !part.is_empty() && *part != ".")
                            .collect::<Vec<_>>()
                            .join("/");
                        if (within.ends_with('/') || within.ends_with("/."))
                            && !normalized.is_empty()
                        {
                            normalized.push('/');
                        }
                        filters.push(format!(":(literal){normalized}"));
                    }
                }
                if !paths.is_empty() && filters.is_empty() {
                    continue;
                }
                let mut arguments = vec!["diff".to_owned()];
                match scope.as_str() {
                    "changed" => {}
                    "staged" => arguments.push("--cached".to_owned()),
                    "unpublished" => {
                        let checkout =
                            checkout.ok_or("this Instance has no checkout declaration")?;
                        let declared = format!("refs/remotes/origin/{}", checkout.branch);
                        let revision =
                            match git(&directory, &["rev-parse", "--verify", &declared]).await {
                                Ok(revision) => revision.trim().to_owned(),
                                Err(_) => git(
                                    &directory,
                                    &[
                                        "merge-base",
                                        "HEAD",
                                        &format!("refs/remotes/origin/{}", checkout.base),
                                    ],
                                )
                                .await?
                                .trim()
                                .to_owned(),
                            };
                        arguments.push(revision);
                    }
                    commit if commit.starts_with("commit:") => {
                        let sha = &commit[7..];
                        if sha.is_empty() || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                            return Err("a commit must be a hexadecimal commit id".to_owned());
                        }
                        let Ok(revision) = git(
                            &directory,
                            &["rev-parse", "--verify", &format!("{sha}^{{commit}}")],
                        )
                        .await
                        else {
                            continue;
                        };
                        arguments = vec![
                            "show".to_owned(),
                            "--format=".to_owned(),
                            "--root".to_owned(),
                            "--diff-merges=first-parent".to_owned(),
                            revision.trim().to_owned(),
                        ];
                    }
                    _ => return Err(format!("unknown changes scope {scope}")),
                }
                arguments.extend(
                    [
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                        "--find-renames",
                    ]
                    .map(str::to_owned),
                );
                let mut stats = arguments.clone();
                stats.extend(["--numstat".to_owned(), "-z".to_owned(), "--".to_owned()]);
                stats.extend(filters.iter().cloned());
                let mut files = file_stats(name, &git_owned(&directory, &stats).await?)?;
                arguments.extend([
                    format!("--src-prefix=a/{name}/"),
                    format!("--dst-prefix=b/{name}/"),
                    "--".to_owned(),
                ]);
                arguments.extend(filters.iter().cloned());
                let (mut diff, mut truncated) =
                    patch(&directory, &arguments, remaining, false).await?;
                if scope == "unpublished" {
                    for path in untracked_paths(&directory).await? {
                        if !filters.is_empty()
                            && !filters.iter().any(|filter| {
                                let filter = filter.strip_prefix(":(literal)").unwrap_or(filter);
                                filter.is_empty()
                                    || path == filter
                                    || path
                                        .starts_with(&format!("{}/", filter.trim_end_matches('/')))
                            })
                        {
                            continue;
                        }
                        let arguments = vec![
                            "diff".to_owned(),
                            "--no-index".to_owned(),
                            "--no-ext-diff".to_owned(),
                            "--no-textconv".to_owned(),
                            "--no-color".to_owned(),
                            format!("--src-prefix=a/{name}/"),
                            format!("--dst-prefix=b/{name}/"),
                            "--".to_owned(),
                            "/dev/null".to_owned(),
                            path,
                        ];
                        let mut stats = arguments.clone();
                        stats.insert(2, "--numstat".to_owned());
                        stats.insert(3, "-z".to_owned());
                        let output = no_index_stats(&directory, &stats).await?;
                        files.extend(file_stats(name, &output)?);
                        let (next, cut) =
                            patch(&directory, &arguments, remaining - diff.len(), true).await?;
                        diff.push_str(&next);
                        truncated |= cut;
                    }
                }
                remaining -= diff.len();
                diffs.push(RepositoryDiff {
                    repository: name.clone(),
                    diff,
                    files,
                    truncated,
                });
            }
            Read::Commits | Read::Stashes => {
                let arguments: &[&str] = if matches!(read, Read::Commits) {
                    &[
                        "log",
                        "--no-color",
                        "HEAD",
                        "--branches",
                        "--not",
                        "--remotes",
                        "--",
                    ]
                } else {
                    &["stash", "list", "--no-color"]
                };
                texts.push(RepositoryText {
                    repository: name.clone(),
                    text: git(&directory, arguments).await?,
                });
            }
            _ => unreachable!(),
        }
    }
    if let Read::Changes { scope, .. } = read
        && scope.starts_with("commit:")
        && diffs.is_empty()
    {
        return Err(format!(
            "{} is not a commit in the selected repositories",
            &scope[7..]
        ));
    }
    Ok(match read {
        Read::Changes { .. } => Answer::Changes {
            repositories: diffs,
        },
        Read::Commits => Answer::Commits {
            repositories: texts,
        },
        _ => Answer::Stashes {
            repositories: texts,
        },
    })
}

async fn untracked_paths(directory: &str) -> Result<Vec<String>, String> {
    let output = git(
        directory,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    let mut pending: Vec<String> = output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        let target = std::path::Path::new(directory).join(&path);
        let metadata = match tokio::fs::symlink_metadata(&target).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{path} could not be read: {error}")),
        };
        if metadata.is_dir() {
            let nested = git(
                &target.to_string_lossy(),
                &[
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                ],
            )
            .await?;
            pending.extend(
                nested
                    .split('\0')
                    .filter(|path| !path.is_empty())
                    .map(|within| format!("{}/{within}", path.trim_end_matches('/'))),
            );
        } else {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

async fn git(directory: &str, arguments: &[&str]) -> Result<String, String> {
    let mut command = vec!["-C", directory];
    command.extend_from_slice(arguments);
    checkout::untrimmed(&command).await
}

async fn git_owned(directory: &str, arguments: &[String]) -> Result<String, String> {
    git(
        directory,
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .await
}

fn command(directory: &str, arguments: &[String]) -> Command {
    let mut command = Command::new("git");
    command
        .args(["-C", directory])
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    command
}

async fn no_index_stats(directory: &str, arguments: &[String]) -> Result<String, String> {
    let output = command(directory, arguments)
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if output.status.success() || (output.status.code() == Some(1) && output.stderr.is_empty()) {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

async fn patch(
    directory: &str,
    arguments: &[String],
    limit: usize,
    no_index: bool,
) -> Result<(String, bool), String> {
    let mut child = command(directory, arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let error = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let mut held = Vec::new();
    let mut buffer = [0; 8192];
    let mut truncated = false;
    loop {
        let count = stdout
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        let taken = count.min(limit - held.len());
        held.extend_from_slice(&buffer[..taken]);
        truncated |= taken < count;
    }
    let status = child.wait().await.map_err(|error| error.to_string())?;
    let error = error
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    if !status.success() && !(no_index && status.code() == Some(1) && error.is_empty()) {
        return Err(String::from_utf8_lossy(&error).into_owned());
    }
    let mut diff = String::from_utf8_lossy(&held).into_owned();
    truncated |= diff.len() > limit;
    while diff.len() > limit {
        diff.pop();
    }
    Ok((diff, truncated))
}

fn file_stats(repository: &str, output: &str) -> Result<Vec<FileStat>, String> {
    let mut files = checkout::file_stats(output)?;
    for file in &mut files {
        file.path = format!("{repository}/{}", file.path);
    }
    Ok(files)
}
