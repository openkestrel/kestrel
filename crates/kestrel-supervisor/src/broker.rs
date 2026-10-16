//! Lends a process about to reach GitHub a token for that one use, asked of the link as it asks
//! (ADR-0028). `git` reaches it as a credential helper and `gh` through a shim; no token is
//! written to a file or kept in an environment anything else inherits.

use std::collections::BTreeMap;
use std::io::{self, BufRead as _, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream as Asking;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::link::{Lent, Link};

/// The socket's path, which the helper and the shim are handed; it lends nothing by itself.
pub const VARIABLE: &str = "KESTREL_BROKER";
/// What a token is presented as over HTTPS, as GitHub documents for an installation token.
const USERNAME: &str = "x-access-token";
const ASKED_AT_MOST: u64 = 64 * 1024;
const PATIENCE: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize)]
struct Asked {
    repository: Option<String>,
}

pub struct Broker {
    directory: PathBuf,
    environment: BTreeMap<String, String>,
    serving: JoinHandle<()>,
}

impl Broker {
    /// Reachable only by this Instance's own user: the directory holding the socket is theirs
    /// alone. `helper` is this binary, which `git` runs as its credential helper.
    pub fn open(
        link: Arc<Link>,
        carried: watch::Receiver<Option<String>>,
        helper: &Path,
        inherited: &BTreeMap<String, String>,
    ) -> io::Result<Self> {
        let directory = std::env::temp_dir().join(format!(
            "kestrel-broker-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().subsec_nanosecond()
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let socket = directory.join("socket");
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        let serving = tokio::spawn(serving(listener, link, carried));

        Ok(Self {
            environment: environment(helper, &socket, inherited),
            directory,
            serving,
        })
    }

    /// For every `git` and `gh` that reaches GitHub on a Session's behalf, the supervisor's own
    /// clone included.
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.serving.abort();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Appended after whatever configuration the Instance already passes in its environment, and
/// clearing every credential helper before it so no other one answers in its place.
pub fn environment(
    helper: &Path,
    socket: &Path,
    inherited: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let first = inherited
        .get("GIT_CONFIG_COUNT")
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or(0);
    let helper = format!(
        "!'{}' credential",
        helper.to_string_lossy().replace('\'', r"'\''")
    );
    let entries = [
        ("credential.helper", ""),
        ("credential.helper", helper.as_str()),
        ("credential.useHttpPath", "true"),
    ];

    let mut environment = BTreeMap::from([
        (VARIABLE.to_owned(), socket.to_string_lossy().into_owned()),
        (
            "GIT_CONFIG_COUNT".to_owned(),
            (first + entries.len()).to_string(),
        ),
    ]);
    for (index, (key, value)) in entries.into_iter().enumerate() {
        environment.insert(format!("GIT_CONFIG_KEY_{}", first + index), key.to_owned());
        environment.insert(
            format!("GIT_CONFIG_VALUE_{}", first + index),
            value.to_owned(),
        );
    }

    environment
}

async fn serving(
    listener: UnixListener,
    link: Arc<Link>,
    carried: watch::Receiver<Option<String>>,
) {
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(lending(stream, Arc::clone(&link), carried.clone()));
    }
}

async fn lending(stream: UnixStream, link: Arc<Link>, carried: watch::Receiver<Option<String>>) {
    let (reading, mut writing) = stream.into_split();
    let mut line = String::new();
    let mut reading =
        tokio::io::BufReader::new(tokio::io::AsyncReadExt::take(reading, ASKED_AT_MOST));
    if reading.read_line(&mut line).await.is_err() {
        return;
    }
    let lent = match serde_json::from_str::<Asked>(&line) {
        Err(error) => Lent::Refused {
            message: format!("the request for a token could not be read: {error}"),
        },
        Ok(asked) => {
            let session = carried.borrow().clone();
            match session {
                None => Lent::Refused {
                    message: "this instance is carrying no session, so nothing reaches github \
                              on one's behalf"
                        .to_owned(),
                },
                Some(session) => link
                    .token(&session, asked.repository.as_deref())
                    .await
                    .unwrap_or_else(|error| Lent::Refused {
                        message: format!("the link lent no token: {error}"),
                    }),
            }
        }
    };
    let Ok(mut answer) = serde_json::to_vec(&lent) else {
        return;
    };
    answer.push(b'\n');
    let _ = writing.write_all(&answer).await;
}

/// Anything that goes wrong reaching the supervisor is a refusal, never a token from elsewhere.
pub fn ask(socket: &Path, repository: Option<&str>) -> Lent {
    let asked = || -> io::Result<Lent> {
        let mut stream = Asking::connect(socket)?;
        stream.set_read_timeout(Some(PATIENCE))?;
        let mut line = serde_json::to_vec(&Asked {
            repository: repository.map(str::to_owned),
        })?;
        line.push(b'\n');
        stream.write_all(&line)?;
        let mut answer = String::new();
        BufReader::new(stream.take(ASKED_AT_MOST)).read_line(&mut answer)?;

        Ok(serde_json::from_str(&answer)?)
    };

    asked().unwrap_or_else(|error| Lent::Refused {
        message: format!("the supervisor could not be asked for a token: {error}"),
    })
}

/// `git credential` helper protocol: a token for `get`, and nothing kept for `store` or `erase`.
pub fn credential(
    action: Option<&str>,
    input: impl Read,
    mut output: impl Write,
    mut errors: impl Write,
    variables: &BTreeMap<String, String>,
) -> i32 {
    let mut described = BTreeMap::new();
    for line in BufReader::new(input.take(ASKED_AT_MOST)).lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            described.insert(key.to_owned(), value.to_owned());
        }
    }
    let (Some("get"), Some(socket)) = (action, set(variables, VARIABLE)) else {
        return 0;
    };
    if described.get("protocol").map(String::as_str) != Some("https") {
        return 0;
    }
    let Some(host) = described.get("host") else {
        return 0;
    };
    let repository = match described.get("path") {
        Some(path) => format!("https://{host}/{}", path.trim_start_matches('/')),
        None => format!("https://{host}"),
    };

    let written = match ask(Path::new(socket), Some(&repository)) {
        Lent::Token { token, expires_at } => writeln!(
            output,
            "username={USERNAME}\npassword={token}\npassword_expiry_utc={}",
            expires_at.as_second()
        ),
        Lent::Unbound => Ok(()),
        // `quit` stops git from asking anything else, so a refusal is never answered elsewhere.
        Lent::Refused { message } => {
            let _ = writeln!(errors, "kestrel: {message}");
            writeln!(output, "quit=1")
        }
    };

    i32::from(written.is_err())
}

/// Runs the real `gh` with a token in its own environment and nowhere else; returns only when it
/// could not be run or was refused one.
pub fn gh(
    real: &str,
    arguments: &[String],
    mut errors: impl Write,
    variables: &BTreeMap<String, String>,
) -> i32 {
    let mut running = Command::new(real);
    running.args(arguments);
    if let Some(socket) = set(variables, VARIABLE) {
        let host = set(variables, "GH_HOST").unwrap_or("github.com");
        let repository = named(arguments)
            .or_else(|| set(variables, "GH_REPO").map(str::to_owned))
            .or_else(origin)
            .and_then(|named| addressed(&named, host));
        match ask(Path::new(socket), repository.as_deref()) {
            Lent::Token { token, .. } => {
                running.env("GH_TOKEN", &token).env_remove("GITHUB_TOKEN");
                if !host.eq_ignore_ascii_case("github.com") {
                    running.env("GH_ENTERPRISE_TOKEN", &token);
                }
            }
            Lent::Unbound => {}
            Lent::Refused { message } => {
                let _ = writeln!(errors, "kestrel: {message}");
                return 1;
            }
        }
    }

    let error = running.exec();
    let _ = writeln!(errors, "kestrel: {real} could not be run: {error}");
    127
}

/// The repository a `gh` command names with `-R` or `--repo`.
fn named(arguments: &[String]) -> Option<String> {
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        if argument == "-R" || argument == "--repo" {
            return arguments.next().cloned();
        }
        if let Some(named) = argument
            .strip_prefix("--repo=")
            .or_else(|| argument.strip_prefix("-R"))
            .filter(|named| !named.is_empty())
        {
            return Some(named.to_owned());
        }
    }

    None
}

/// As configured, before any `insteadOf` rewrites it: the address the repository was declared by.
fn origin() -> Option<String> {
    let ran = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    let url = String::from_utf8(ran.stdout).ok()?;

    ran.status.success().then(|| url.trim().to_owned())
}

/// The HTTPS address of a repository in any form `gh` or `git` names one by.
fn addressed(named: &str, host: &str) -> Option<String> {
    if let Some((scheme, rest)) = named.split_once("://") {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
        return Some(match scheme {
            "ssh" | "git" => format!("https://{}", rest.replacen(':', "/", 1)),
            _ => format!("{scheme}://{rest}"),
        });
    }
    if let Some((before, path)) = named.split_once(':')
        && !before.contains('/')
    {
        let host = before.rsplit_once('@').map_or(before, |(_, host)| host);
        return Some(format!("https://{host}/{path}"));
    }
    match named.split('/').filter(|part| !part.is_empty()).count() {
        2 => Some(format!("https://{host}/{named}")),
        3 => Some(format!("https://{named}")),
        _ => None,
    }
}

fn set<'a>(variables: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    variables
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|&argument| argument.to_owned())
            .collect()
    }

    #[test]
    fn the_repository_a_gh_command_names_is_found_in_any_of_its_spellings() {
        for arguments in [
            &["pr", "create", "-R", "jtmthf/kestrel"][..],
            &["pr", "create", "-Rjtmthf/kestrel"],
            &["pr", "create", "--repo", "jtmthf/kestrel"],
            &["pr", "create", "--repo=jtmthf/kestrel", "--fill"],
        ] {
            assert_eq!(
                named(&strings(arguments)).as_deref(),
                Some("jtmthf/kestrel"),
                "{arguments:?}"
            );
        }
        assert_eq!(named(&strings(&["pr", "create", "--", "-R", "x/y"])), None);
        assert_eq!(named(&strings(&["pr", "list"])), None);
    }

    #[test]
    fn a_repository_is_asked_for_by_its_https_address_whatever_form_it_was_named_in() {
        let https = Some("https://github.com/jtmthf/kestrel.git".to_owned());

        assert_eq!(
            addressed("git@github.com:jtmthf/kestrel.git", "github.com"),
            https
        );
        assert_eq!(
            addressed("ssh://git@github.com/jtmthf/kestrel.git", "github.com"),
            https
        );
        assert_eq!(
            addressed("https://github.com/jtmthf/kestrel.git", "github.com"),
            https
        );
        assert_eq!(
            addressed("jtmthf/kestrel", "github.com").as_deref(),
            Some("https://github.com/jtmthf/kestrel")
        );
        assert_eq!(
            addressed("jtmthf/kestrel", "ghe.example.com").as_deref(),
            Some("https://ghe.example.com/jtmthf/kestrel")
        );
        assert_eq!(
            addressed("ghe.example.com/jtmthf/kestrel", "github.com").as_deref(),
            Some("https://ghe.example.com/jtmthf/kestrel")
        );
        assert_eq!(addressed("kestrel", "github.com"), None);
    }

    #[test]
    fn git_is_configured_after_what_the_instance_already_passes_and_with_no_other_helper() {
        let inherited = BTreeMap::from([("GIT_CONFIG_COUNT".to_owned(), "1".to_owned())]);

        let environment = environment(
            Path::new("/usr/local/bin/it's-here"),
            Path::new("/tmp/broker/socket"),
            &inherited,
        );

        assert_eq!(
            environment,
            BTreeMap::from(
                [
                    (VARIABLE, "/tmp/broker/socket"),
                    ("GIT_CONFIG_COUNT", "4"),
                    ("GIT_CONFIG_KEY_1", "credential.helper"),
                    ("GIT_CONFIG_VALUE_1", ""),
                    ("GIT_CONFIG_KEY_2", "credential.helper"),
                    (
                        "GIT_CONFIG_VALUE_2",
                        r"!'/usr/local/bin/it'\''s-here' credential"
                    ),
                    ("GIT_CONFIG_KEY_3", "credential.useHttpPath"),
                    ("GIT_CONFIG_VALUE_3", "true"),
                ]
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
            )
        );
    }

    #[test]
    fn a_helper_handed_no_broker_or_a_protocol_other_than_https_answers_nothing() {
        let asked = "protocol=https\nhost=github.com\npath=jtmthf/kestrel.git\n\n";
        let mut output = Vec::new();

        assert_eq!(
            credential(
                Some("get"),
                asked.as_bytes(),
                &mut output,
                io::sink(),
                &BTreeMap::new()
            ),
            0
        );
        let broker = BTreeMap::from([(VARIABLE.to_owned(), "/nowhere".to_owned())]);
        assert_eq!(
            credential(
                Some("get"),
                "protocol=http\nhost=github.com\n\n".as_bytes(),
                &mut output,
                io::sink(),
                &broker
            ),
            0
        );
        assert_eq!(
            credential(
                Some("store"),
                asked.as_bytes(),
                &mut output,
                io::sink(),
                &broker
            ),
            0
        );
        assert!(output.is_empty());
    }

    #[test]
    fn a_helper_that_cannot_reach_the_supervisor_stops_git_asking_anywhere_else() {
        let broker = BTreeMap::from([(VARIABLE.to_owned(), "/nowhere/socket".to_owned())]);
        let (mut output, mut errors) = (Vec::new(), Vec::new());

        credential(
            Some("get"),
            "protocol=https\nhost=github.com\npath=jtmthf/kestrel.git\n\n".as_bytes(),
            &mut output,
            &mut errors,
            &broker,
        );

        assert_eq!(String::from_utf8_lossy(&output), "quit=1\n");
        assert!(String::from_utf8_lossy(&errors).starts_with("kestrel: the supervisor could not"));
    }
}
