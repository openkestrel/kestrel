use crate::support::client;

#[test]
fn each_shell_installs_and_invokes_the_dynamic_engine_offline() {
    for shell in ["bash", "zsh", "fish"] {
        let script = client::ran("http://127.0.0.1:1", &["completions", shell]);
        assert!(script.status.success(), "{}", script.err);
        assert!(script.out.join("\n").contains("COMPLETE="));
        let completed = client::ran_as(
            "http://127.0.0.1:1",
            &["--", "kestrel", "sta"],
            client::Invocation::default()
                .env("COMPLETE", shell)
                .env("_CLAP_COMPLETE_INDEX", "1")
                .env("_CLAP_IFS", "\n"),
        );
        assert!(completed.status.success(), "{}", completed.err);
        assert!(completed.err.is_empty());
        assert!(completed.out.join("\n").contains("start"));
        assert!(completed.left_behind.is_empty());
    }
}

use crate::support::{Kestrel, fixture::Fixture};
use client::{Finished, Invocation};

fn engine(shell: &str, line: &[&str], invocation: Invocation) -> Invocation {
    invocation
        .env("COMPLETE", shell)
        .env(
            "_CLAP_COMPLETE_INDEX",
            &line
                .iter()
                .position(|word| word.is_empty())
                .unwrap_or(line.len() - 1)
                .to_string(),
        )
        .env("_CLAP_IFS", "\n")
}

async fn complete(
    kestrel: &Kestrel,
    shell: &str,
    line: &[&str],
    invocation: Invocation,
) -> Finished {
    let args: Vec<_> = ["--"].into_iter().chain(line.iter().copied()).collect();
    let finished = client::ran_by(kestrel, &args, engine(shell, line, invocation)).await;
    assert!(finished.status.success(), "{}", finished.err);
    assert!(finished.err.is_empty(), "{}", finished.err);
    assert!(
        finished.left_behind.is_empty(),
        "{:?}",
        finished.left_behind
    );
    finished
}

#[tokio::test]
async fn names_follow_scope_precedence_and_selected_harness_in_every_shell() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let fixture = Fixture::acme().harness("claude");
    let workspace = fixture.open(&kestrel).await;
    kestrel
        .declare_profile("acme", "personal", "Ada")
        .await
        .unwrap();
    for shell in ["bash", "zsh", "fish"] {
        for (line, expected) in [
            (
                vec!["kestrel", "workspace", "show", ""],
                workspace.name.as_str(),
            ),
            (
                vec!["kestrel", "workspace", "open", "--agent", "bui"],
                "builder",
            ),
            (vec!["kestrel", "agent", "model", ""], "builder"),
            (
                vec!["kestrel", "workspace", "open", "--profile", ""],
                "personal",
            ),
            (vec!["kestrel", "profile", "forget", ""], "personal"),
            (vec!["kestrel", "login", ""], "claude"),
            (
                vec!["kestrel", "login", "claude", "--method", ""],
                "claude-setup-token",
            ),
            (
                vec!["kestrel", "sign-in", "test", "codex", "--method", ""],
                "codex-device-auth",
            ),
            (
                vec![
                    "kestrel",
                    "workspace",
                    "open",
                    "--agent",
                    "builder",
                    "--sign-in",
                    "",
                ],
                "claude-setup-token",
            ),
        ] {
            let result = complete(&kestrel, shell, &line, Invocation::default()).await;
            let said = result.out.join("\n");
            assert!(said.contains(expected), "{shell} {line:?}: {said}");
            if line.contains(&"--method") || line.contains(&"--sign-in") {
                let foreign = if expected == "codex-device-auth" {
                    "claude-setup-token"
                } else {
                    "codex-device-auth"
                };
                assert!(!said.contains(foreign), "{said}");
            }
        }
    }
    for shell in ["bash", "zsh", "fish"] {
        let result = complete(
            &kestrel,
            shell,
            &[
                "kestrel",
                "session",
                "enqueue",
                "--workspace",
                &workspace.name,
                "--sign-in",
                "",
            ],
            Invocation::default(),
        )
        .await;
        assert!(
            result.out.join("\n").contains("claude-setup-token"),
            "{:?}",
            result.out
        );
        assert!(!result.out.join("\n").contains("codex-device-auth"));
    }
    Fixture::acme()
        .organization("other")
        .agent_name("outsider")
        .declare(&kestrel)
        .await;
    for shell in ["bash", "zsh", "fish"] {
        for line in [
            vec!["kestrel", "--organization", "absent", "login", ""],
            vec![
                "kestrel",
                "--organization=absent",
                "login",
                "claude",
                "--method",
                "",
            ],
        ] {
            let result = complete(&kestrel, shell, &line, Invocation::default()).await;
            assert!(
                !result.out.iter().any(|item| item.contains("claude")),
                "unavailable scope: {:?}",
                result.out
            );
        }
        let line = ["kestrel", "workspace", "open", "--agent", ""];
        let ambiguous = complete(&kestrel, shell, &line, Invocation::default()).await;
        assert!(ambiguous.out.is_empty(), "{:?}", ambiguous.out);
        for invocation in [
            Invocation::default().env("KESTREL_ORGANIZATION", "acme"),
            Invocation::default()
                .file(".git/HEAD", "ref: refs/heads/main\n")
                .file(".kestrel/organization", "acme\n")
                .within("nested"),
        ] {
            let result = complete(&kestrel, shell, &line, invocation).await;
            assert!(result.out.join("\n").contains("builder"));
            assert!(!result.out.join("\n").contains("outsider"));
        }
        for line in [
            vec![
                "kestrel",
                "--organization",
                "acme",
                "workspace",
                "open",
                "--agent",
                "",
            ],
            vec![
                "kestrel",
                "workspace",
                "open",
                "--organization=acme",
                "--agent",
                "",
            ],
        ] {
            let result = complete(
                &kestrel,
                shell,
                &line,
                Invocation::default()
                    .env("KESTREL_ORGANIZATION", "other")
                    .env("KESTREL_CONTROL_PLANE", "http://127.0.0.1:1"),
            )
            .await;
            assert!(result.out.is_empty());
            let mut overridden = line.clone();
            let operator = kestrel.operator();
            overridden.splice(1..1, ["--control-plane", operator.as_str()]);
            let result = complete(
                &kestrel,
                shell,
                &overridden,
                Invocation::default()
                    .env("KESTREL_ORGANIZATION", "other")
                    .env("KESTREL_CONTROL_PLANE", "http://127.0.0.1:1"),
            )
            .await;
            assert!(
                result.out.join("\n").contains("builder"),
                "{:?}",
                result.out
            );
            assert!(!result.out.join("\n").contains("outsider"));
        }
    }
    kestrel.teardown().await;
}

#[test]
fn hung_or_unavailable_remote_reads_are_silent_and_bounded_in_every_shell() {
    use std::net::TcpListener;
    use std::time::{Duration, Instant};
    assert!(
        client::ran("http://127.0.0.1:1", &["exit-codes"])
            .status
            .success()
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let hung = format!("http://{}", listener.local_addr().unwrap());
    for shell in ["bash", "zsh", "fish"] {
        for url in [hung.as_str(), "http://127.0.0.1:1"] {
            let line = ["kestrel", "login", "claude", "--method", ""];
            let mut child = client::Client::spawn_as(
                url,
                &["--", "kestrel", "login", "claude", "--method", ""],
                engine(shell, &line, Invocation::default()),
            );
            let started = Instant::now();
            let result = child.finish();
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "{shell} {url}: {} ms",
                started.elapsed().as_millis()
            );
            assert!(result.status.success());
            assert!(result.out.is_empty(), "{:?}", result.out);
            assert!(result.err.is_empty(), "{}", result.err);
            assert!(result.left_behind.is_empty());
        }
    }
}

#[tokio::test]
async fn scope_flags_after_the_cursor_are_read_and_incomplete_scope_is_silent() {
    let kestrel = Kestrel::boot_serving_alone().await;
    Fixture::acme().declare(&kestrel).await;
    Fixture::acme()
        .organization("other")
        .agent_name("outsider")
        .declare(&kestrel)
        .await;
    for shell in ["bash", "zsh"] {
        let result = complete(
            &kestrel,
            shell,
            &[
                "kestrel",
                "workspace",
                "open",
                "--agent",
                "",
                "--organization",
                "acme",
            ],
            Invocation::default().env("KESTREL_ORGANIZATION", "other"),
        )
        .await;
        assert!(
            result.out.join("\n").contains("builder"),
            "{:?}",
            result.out
        );
        for tail in [
            vec!["--organization"],
            vec!["--organization="],
            vec!["--control-plane"],
        ] {
            let line: Vec<_> = ["kestrel", "workspace", "open", "--agent", ""]
                .into_iter()
                .chain(tail)
                .collect();
            let result = complete(
                &kestrel,
                shell,
                &line,
                Invocation::default().env("KESTREL_ORGANIZATION", "acme"),
            )
            .await;
            assert!(
                result.out.is_empty(),
                "incomplete {line:?}: {:?}",
                result.out
            );
        }
    }
    kestrel.teardown().await;
}

#[test]
fn scope_and_stalled_response_body_share_one_deadline() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    assert!(
        client::ran("http://127.0.0.1:1", &["exit-codes"])
            .status
            .success()
    );
    for shell in ["bash", "zsh", "fish"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut paths = Vec::new();
            loop {
                let (mut stream, _) = match listener.accept() {
                    Ok(accepted) => accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "completion did not read the catalogue"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                paths.push(
                    String::from_utf8(request)
                        .unwrap()
                        .lines()
                        .next()
                        .unwrap()
                        .to_owned(),
                );
                if paths.len() == 1 {
                    std::thread::sleep(Duration::from_millis(200));
                    let body = r#"[{"id":"00000000-0000-0000-0000-000000000001","name":"acme","max_live_instances":null}]"#;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                } else {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n[")
                        .unwrap();
                    return (stream, paths);
                }
            }
        });
        let line = ["kestrel", "login", "claude", "--method", ""];
        let mut child = client::Client::spawn_as(
            &url,
            &["--", "kestrel", "login", "claude", "--method", ""],
            engine(shell, &line, Invocation::default()),
        );
        let started = Instant::now();
        let result = child.finish();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{shell}: {:?}",
            started.elapsed()
        );
        assert!(result.status.success());
        assert!(result.out.is_empty());
        assert!(result.err.is_empty(), "{}", result.err);
        assert!(result.left_behind.is_empty());
        let (_held, paths) = server.join().unwrap();
        assert_eq!(
            paths,
            [
                "GET /operator/organizations HTTP/1.1",
                "GET /operator/harnesses HTTP/1.1"
            ]
        );
    }
}
