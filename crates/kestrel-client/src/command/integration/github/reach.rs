//! How a browser reaches the Client origin a GitHub App setup URL names: opened here, or over
//! an SSH tunnel from the machine the person types on.

use std::process::{Command, Stdio};

use reqwest::Url;

use crate::shell::quoted;

const TEMPLATE: &str = "<ssh-destination>";

/// What is known of where the person is, read once from the environment.
pub(super) struct Surroundings {
    ssh: bool,
    /// `SSH_CONNECTION`'s server address and port: where the person's ssh connected to.
    server: Option<(String, u16)>,
    user: Option<String>,
    display: bool,
}

impl Surroundings {
    pub(super) fn read() -> Self {
        let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        let server = std::env::var("SSH_CONNECTION").ok().and_then(|connection| {
            let fields: Vec<&str> = connection.split_whitespace().collect();
            match fields.as_slice() {
                [_, _, address, port] => Some(((*address).to_owned(), port.parse().ok()?)),
                _ => None,
            }
        });
        let user = ["USER", "LOGNAME"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|user| !user.is_empty()));
        Self {
            ssh: server.is_some() || set("SSH_CLIENT") || set("SSH_TTY"),
            server,
            user,
            display: cfg!(target_os = "macos") || set("DISPLAY") || set("WAYLAND_DISPLAY"),
        }
    }
}

pub(super) enum Reach {
    Here,
    /// The browser is elsewhere; `known` is false when the destination is a placeholder.
    Tunnel {
        command: String,
        known: bool,
    },
}

pub(super) fn reach(surroundings: &Surroundings, client: &Url) -> Reach {
    if !surroundings.ssh && surroundings.display {
        return Reach::Here;
    }
    let port = client.port_or_known_default().unwrap_or(80);
    let host = client.host_str().unwrap_or("localhost");
    let destination = match (&surroundings.server, &surroundings.user) {
        (Some((address, port)), Some(user)) if surroundings.ssh => {
            let at = format!("{}@{address}", quoted(user));
            Some(if *port == 22 {
                at
            } else {
                format!("-p {port} {at}")
            })
        }
        _ => None,
    };
    Reach::Tunnel {
        known: destination.is_some(),
        command: format!(
            "ssh -N -L {port}:{host}:{port} {}",
            destination.as_deref().unwrap_or(TEMPLATE)
        ),
    }
}

pub(super) fn shown(url: &str, reach: &Reach, interactive: bool) {
    match reach {
        Reach::Here => {
            eprintln!("Open this URL to create and install the GitHub App:");
            eprintln!("  {url}");
            if interactive {
                opened(url);
            }
        }
        Reach::Tunnel { command, known } => {
            eprintln!("GitHub setup needs a browser that can reach kestrel.");
            if *known {
                eprintln!("In a second terminal on your laptop, run:");
            } else {
                eprintln!(
                    "In a second terminal on your laptop, run this template with your own SSH \
                     destination in place of {TEMPLATE}:"
                );
            }
            eprintln!("  {command}");
            eprintln!("Then open this URL on your laptop:");
            eprintln!("  {url}");
        }
    }
}

/// Best effort: the URL is already printed, so a missing opener changes nothing.
fn opened(url: &str) {
    let opener = std::env::var("BROWSER")
        .ok()
        .filter(|browser| !browser.is_empty())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            }
            .to_owned()
        });
    let _ = Command::new(opener)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
