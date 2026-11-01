use crate::support;

use std::time::Duration;

use serde_json::Value;
use support::client::{self, Finished, Invocation};
use support::scripted_agent::{self, Script};

pub(super) struct Booted {
    pub plane: support::Kestrel,
    operator: String,
    log: Option<support::operator_log::Trace>,
}

impl Booted {
    pub async fn playing(script: Script) -> Self {
        let plane = support::Kestrel::stepped_dispatching_to(
            support::supervisor::binary(),
            &scripted_agent::playing(script),
        )
        .await;
        Self {
            operator: plane.operator(),
            plane,
            log: None,
        }
    }

    pub async fn traced() -> Self {
        let log = support::operator_log::capturing_trace();
        let mut booted = Self::playing(Script::Speaks).await;
        booted.log = Some(log);
        booted
    }

    pub async fn client(&self, args: &[&str]) -> Finished {
        self.client_as(args, Invocation::default()).await
    }

    pub async fn client_as(&self, args: &[&str], invocation: Invocation) -> Finished {
        client::ran_by(&self.plane, args, invocation).await
    }

    pub async fn run(&self, args: &[&str]) -> String {
        succeeded(args, &self.client(args).await)
    }

    pub async fn run_as(&self, args: &[&str], invocation: Invocation) -> String {
        succeeded(args, &self.client_as(args, invocation).await)
    }

    pub async fn records(&self, args: &[&str]) -> Vec<Value> {
        self.client(args).await.records()
    }

    pub async fn record(&self, args: &[&str]) -> Value {
        self.client(args).await.json()
    }

    pub async fn refused(&self, args: &[&str]) -> String {
        refusal(args, &self.client(args).await)
    }

    pub async fn refused_as(&self, args: &[&str], invocation: Invocation) -> String {
        refusal(args, &self.client_as(args, invocation).await)
    }

    pub async fn until(
        &self,
        args: &[&str],
        listed: impl Fn(&[Value]) -> bool,
        what: &str,
    ) -> Vec<Value> {
        let mut shown = Vec::new();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                shown = self.records(args).await;
                if listed(&shown) {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("`kestrel {}` never {what}: {shown:?}", args.join(" ")));
        shown
    }

    pub async fn terminal(&self, args: &[&str]) -> client::Shown {
        let operator = self.operator.clone();
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        tokio::task::spawn_blocking(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            client::ran_on_a_terminal(&operator, &args, 120, "")
        })
        .await
        .expect("the terminal Client should run")
    }

    pub fn said(&self) -> String {
        self.log.as_ref().expect("tracing should be enabled").said()
    }
}

pub(super) fn succeeded(args: &[&str], finished: &Finished) -> String {
    assert!(
        finished.status.success(),
        "`kestrel {}` failed with {}:\n{}",
        args.join(" "),
        finished.status,
        finished.err
    );
    finished.out.join("\n")
}

pub(super) fn refusal(args: &[&str], finished: &Finished) -> String {
    assert!(
        !finished.status.success(),
        "`kestrel {}` was expected to be refused, and succeeded",
        args.join(" ")
    );
    finished.err.clone()
}

pub(super) async fn dispatched(kestrel: &Booted, workspace: &str) -> Vec<Value> {
    kestrel.plane.work_pass().await;
    let listed = kestrel
        .until(
            &["session", "list", "--workspace", workspace, "--json"],
            |listed| listed.iter().all(|session| session["state"] == "waiting"),
            "answer its turn",
        )
        .await;
    assert!(!listed.is_empty(), "the Workspace should have a Session");
    for session in &listed {
        let id = session["id"].as_str().expect("the Session identifier");
        kestrel.run(&["session", "stop", id]).await;
    }
    kestrel.plane.work_pass().await;
    kestrel
        .records(&["session", "list", "--workspace", workspace, "--json"])
        .await
}
