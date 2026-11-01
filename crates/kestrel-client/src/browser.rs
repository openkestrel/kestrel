use std::process::{Command, Stdio};

use reqwest::Url;

use crate::shell::quoted;

const TEMPLATE: &str = "<ssh-destination>";

/// What is known of where the person is, read once from the environment.
pub(crate) struct Surroundings {
    ssh: bool,
    /// `SSH_CONNECTION`'s server address and port: where the person's ssh connected to.
    server: Option<(String, u16)>,
    user: Option<String>,
    display: bool,
}

impl Surroundings {
    pub(crate) fn read() -> Self {
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

pub(crate) enum Reach {
    Here,
    /// The browser is elsewhere; `known` is false when the destination is a placeholder.
    Tunnel {
        command: String,
        known: bool,
    },
}

pub(crate) fn reach(surroundings: &Surroundings, client: &Url, operator: Option<&Url>) -> Reach {
    if !surroundings.ssh && surroundings.display {
        return Reach::Here;
    }
    let port = client.port_or_known_default().unwrap_or(80);
    let host = client.host_str().unwrap_or("localhost");
    let mut forwards = format!("-L {port}:{host}:{port}");
    if let Some(operator) = operator
        && let (Some(host), Some(operator_port)) =
            (operator.host_str(), operator.port_or_known_default())
        && operator_port != port
    {
        forwards.push_str(&format!(" -L {operator_port}:{host}:{operator_port}"));
    }
    let (destination, known) = match (&surroundings.server, &surroundings.user) {
        (Some((address, port)), user) if surroundings.ssh => {
            let user = user
                .as_deref()
                .map(quoted)
                .unwrap_or_else(|| "<ssh-user>".to_owned());
            let at = format!("{user}@{address}");
            (
                if *port == 22 {
                    at
                } else {
                    format!("-p {port} {at}")
                },
                surroundings.user.is_some(),
            )
        }
        (None, Some(user)) => (format!("{}@<ssh-host>", quoted(user)), false),
        _ => (TEMPLATE.to_owned(), false),
    };
    Reach::Tunnel {
        known,
        command: format!("ssh -N {forwards} {destination}"),
    }
}

pub(crate) fn shown(url: &str, reach: &Reach, interactive: bool) {
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
                    "In a second terminal on your laptop, replace the <ssh-...> placeholders in this template:"
                );
            }
            eprintln!("  {command}");
            eprintln!("Then open this URL on your laptop:");
            eprintln!("  {url}");
        }
    }
}

/// Best effort: the URL is already printed, so a missing opener changes nothing.
pub(crate) fn opened(url: &str) {
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

pub(crate) fn access(reach: Reach) {
    if let Reach::Tunnel { command, known } = reach {
        if known {
            eprintln!("From your laptop, forward the Client and operator ports:");
        } else {
            eprintln!(
                "For remote access, replace the <ssh-...> placeholders in this SSH template:"
            );
        }
        eprintln!("  {command}");
        eprintln!("Then open the printed URL in your browser.");
    }
}
