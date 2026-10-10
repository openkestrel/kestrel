use kestrel_operator_types as wire;

use super::{Invocation, next_steps};
use crate::BINARY;
use crate::shell::quoted;

pub struct Step {
    pub says: String,
    pub command: Option<Vec<Word>>,
    organization: Option<String>,
    /// Destructive: shown, and taken only on an explicit yes.
    pub consequence: Option<String>,
    /// The invocation's own arguments, which already name their scope and control plane.
    repeated: bool,
}

pub enum Word {
    Given(String),
    Asked(Input),
}

pub struct Input {
    pub name: String,
    pub flag: Option<String>,
}

impl Input {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            flag: flag_for(name),
        }
    }

    fn placeholder(&self) -> String {
        format!("<{}>", self.name.to_uppercase())
    }

    fn needed(&self) -> String {
        self.flag.clone().unwrap_or_else(|| self.placeholder())
    }
}

/// The CLI's bindings for an action's fixed input vocabulary; a name is positional.
fn flag_for(input: &str) -> Option<String> {
    match input {
        "name" => None,
        "repositories" | "repository" => Some("--repository".to_owned()),
        other => Some(format!("--{}", other.replace('_', "-"))),
    }
}

impl Step {
    fn said(says: impl Into<String>) -> Self {
        Self {
            says: says.into(),
            command: None,
            organization: None,
            consequence: None,
            repeated: false,
        }
    }

    fn run(says: impl Into<String>, command: Vec<Word>, organization: Option<String>) -> Self {
        Self {
            command: Some(command),
            organization,
            ..Self::said(says)
        }
    }

    /// A shown command names the control plane only when it is not the default; a launched one
    /// always does, because its environment may not be the one the person types into.
    pub fn scope(&self, invocation: &Invocation, launched: bool) -> Vec<String> {
        if self.repeated {
            return Vec::new();
        }
        let mut words = Vec::new();
        if let Some(organization) = &self.organization {
            words.extend(["--organization".to_owned(), organization.clone()]);
        }
        if invocation.elsewhere || launched {
            words.extend([
                "--control-plane".to_owned(),
                invocation.control_plane.clone(),
            ]);
        }
        words
    }

    fn shown(&self, command: &[Word], invocation: &Invocation) -> String {
        let mut shown = vec![BINARY.to_owned()];
        for word in command {
            match word {
                Word::Given(value) => shown.push(quoted(value)),
                Word::Asked(input) => {
                    shown.extend(input.flag.clone());
                    shown.push(input.placeholder());
                }
            }
        }
        shown.extend(
            self.scope(invocation, false)
                .iter()
                .map(|word| quoted(word)),
        );
        shown.join(" ")
    }
}

pub fn described(diagnostic: &wire::Diagnostic, steps: &[Step], invocation: &Invocation) -> String {
    let mut said = String::new();
    for fact in facts(diagnostic) {
        said.push_str(&format!("  {fact}\n"));
    }
    if steps.is_empty() {
        return said;
    }
    said.push_str("next:\n");
    let mut number = 0;
    for step in steps {
        let Some(command) = &step.command else {
            said.push_str(&format!("  -  {}\n", step.says));
            continue;
        };
        number += 1;
        said.push_str(&format!(
            "  {number}. {}\n     {}\n",
            step.says,
            step.shown(command, invocation)
        ));
        let needed: Vec<String> = command
            .iter()
            .filter_map(|word| match word {
                Word::Asked(input) => Some(input.needed()),
                Word::Given(_) => None,
            })
            .collect();
        if !needed.is_empty() {
            said.push_str(&format!("     needs {}\n", needed.join(" and ")));
        }
        if let Some(consequence) = &step.consequence {
            said.push_str(&format!(
                "     {consequence} It happens only if you choose it.\n"
            ));
        }
    }
    said
}

fn facts(diagnostic: &wire::Diagnostic) -> Vec<String> {
    use wire::Diagnostic as D;
    match diagnostic {
        D::AuthenticationFailedDiagnostic(d) => {
            let context = &d.context;
            let mut facts = vec![ran_by(&context.harness, context.image.as_deref())];
            if let Some(sign_in) = &context.sign_in {
                facts.push(format!("signed in with {sign_in}"));
            }
            facts.push(match &context.evidence.method {
                Some(method) => format!(
                    "the harness asked for authentication (code {}) by {method}",
                    context.evidence.code
                ),
                None => format!(
                    "the harness asked for authentication (code {})",
                    context.evidence.code
                ),
            });
            if context.expired == Some(true) {
                facts.push("the sign-in has expired".to_owned());
            }
            if context.covered == Some(false) {
                facts.push("the sign-in does not cover this harness".to_owned());
            }
            facts
        }
        D::CredentialRejectedDiagnostic(d) => {
            let context = &d.context;
            let mut facts = vec![format!(
                "{} refused the {} key for harness {}",
                context.provider, context.method, context.harness
            )];
            if let Some(status) = context.status {
                facts.push(format!("the provider answered {status}"));
            }
            if let Some(error) = &context.provider_error {
                facts.push(format!("the provider said {error}"));
            }
            facts
        }
        D::ExecutableMissingDiagnostic(d) => vec![
            ran_by(&d.context.harness, d.context.image.as_deref()),
            format!(
                "`{}` could not run: {}",
                d.context.evidence.command,
                d.context.evidence.error.kind.as_str()
            ),
        ],
        D::UnknownFailureDiagnostic(d) => d
            .context
            .evidence
            .iter()
            .map(|evidence| evidence.summary.clone())
            .collect(),
        D::UnknownResponseDiagnostic(d) => d.context.evidence.iter().cloned().collect(),
        D::ClientFailureDiagnostic(d) => d.context.evidence.iter().cloned().collect(),
        D::UnavailableDiagnostic(d) => d
            .context
            .retry_after_seconds
            .map(|seconds| format!("retry after {seconds} seconds"))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn ran_by(harness: &str, image: Option<&str>) -> String {
    match image {
        Some(image) => format!("harness {harness} in image {image}"),
        None => format!("harness {harness}"),
    }
}

fn given(words: &[&str]) -> Vec<Word> {
    words
        .iter()
        .map(|word| Word::Given((*word).to_owned()))
        .collect()
}

fn held_or_asked(
    flag: Option<&str>,
    value: Option<&str>,
    name: &str,
    missing: &[String],
) -> Vec<Word> {
    match value {
        Some(value) if !missing.iter().any(|input| input == name) => flag
            .map(|flag| Word::Given(flag.to_owned()))
            .into_iter()
            .chain([Word::Given(value.to_owned())])
            .collect(),
        _ => vec![Word::Asked(Input::named(name))],
    }
}

/// `wrote` withholds re-running the invocation: a write it already sent would go again.
pub fn steps(diagnostic: &wire::Diagnostic, invocation: &Invocation, wrote: bool) -> Vec<Step> {
    let mut steps = Vec::new();
    if let wire::Diagnostic::AmbiguousReferenceDiagnostic(d) = diagnostic {
        for candidate in &d.context.candidates {
            let (command, organization) = inspection(
                &d.context.resource,
                Some(&candidate.id),
                d.context.organization.as_deref(),
            );
            steps.push(Step::run(
                format!(
                    "inspect the {} {} ({})",
                    noun(&d.context.resource),
                    candidate.name,
                    candidate.id
                ),
                command,
                organization,
            ));
        }
    }
    steps.extend(
        next_steps(diagnostic)
            .iter()
            .map(|action| step(action, invocation, wrote)),
    );
    steps
}

fn step(action: &wire::Action, invocation: &Invocation, wrote: bool) -> Step {
    use wire::Action as A;
    match action {
        A::NameOperatorAction(_) => Step::said("name the Operator, then try again"),
        A::DeclareOrganizationAction(a) => {
            let mut command = given(&["organization", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            Step::run("declare the Organization", command, None)
        }
        A::DeclareProjectAction(a) => {
            let mut command = given(&["project", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            match &a.repositories {
                Some(repositories)
                    if !repositories.is_empty()
                        && !a.missing.iter().any(|input| input == "repositories") =>
                {
                    for repository in repositories {
                        command.extend(given(&["--repository", repository]));
                    }
                }
                _ => command.push(Word::Asked(Input::named("repository"))),
            }
            command.extend(held_or_asked(
                Some("--branch"),
                a.branch.as_deref(),
                "branch",
                &a.missing,
            ));
            Step::run("declare the Project", command, Some(a.organization.clone()))
        }
        A::DeclareAgentAction(a) => {
            let mut command = given(&["agent", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            command.extend(held_or_asked(
                Some("--harness"),
                a.harness.as_deref(),
                "harness",
                &a.missing,
            ));
            Step::run("declare the Agent", command, Some(a.organization.clone()))
        }
        A::DeclareSubscriptionProfileAction(a) => {
            let mut command = given(&["profile", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            command.extend(held_or_asked(
                Some("--owner"),
                a.owner.as_deref(),
                "owner",
                &a.missing,
            ));
            Step::run(
                "declare the Subscription Profile",
                command,
                Some(a.organization.clone()),
            )
        }
        A::SetProviderCredentialAction(a) => Step::run(
            format!(
                "hold the Provider Credential {}; its value is read from standard input, unechoed",
                a.name
            ),
            given(&["credential", "set", &a.name]),
            Some(a.organization.clone()),
        ),
        A::InspectResourceAction(a) => {
            let (command, organization) = inspection(
                &a.resource,
                a.reference.as_deref(),
                a.organization.as_deref(),
            );
            let says = match &a.reference {
                Some(reference) => format!("inspect the {} {reference}", noun(&a.resource)),
                None => format!("list the {}s", noun(&a.resource)),
            };
            Step::run(says, command, organization)
        }
        A::ListResourcesAction(a) => {
            let (command, organization) = listing(&a.resource, a.organization.as_deref());
            Step::run(
                format!("list the {}s", noun(&a.resource)),
                command,
                organization,
            )
        }
        A::StopSessionAction(a) => Step {
            consequence: Some(a.consequence.clone()),
            ..Step::run(
                format!("stop the Session {}", a.session),
                given(&["session", "stop", &a.session]),
                Some(a.organization.clone()),
            )
        },
        A::EnqueueSessionAction(a) => {
            let mut command = given(&["session", "enqueue", "--workspace", &a.workspace]);
            command.extend(
                a.missing
                    .iter()
                    .map(|input| Word::Asked(Input::named(input))),
            );
            Step::run(
                format!("enqueue a Session in the Workspace {}", a.workspace),
                command,
                Some(a.organization.clone()),
            )
        }
        A::EnableIntegrationAction(a) => Step::run(
            format!("enable the Integration {}", a.integration),
            given(&["integration", "enable", &a.integration]),
            Some(a.organization.clone()),
        ),
        A::ReplaceIntegrationPrivateKeyAction(a) => {
            let wait = match a.retry_after_seconds {
                Some(seconds) if seconds > 0 => format!(" after {seconds}s"),
                _ => String::new(),
            };
            Step::run(
                format!(
                    "replace the Integration {}'s App private key{wait}; it is read from \
                     standard input, unechoed, or from --private-key-file",
                    a.integration
                ),
                given(&["integration", "github", "replace-key", &a.integration]),
                Some(a.organization.clone()),
            )
        }
        A::ReleaseInstanceAction(a) => Step {
            consequence: Some(a.consequence.clone()),
            ..Step::run(
                format!("release the Workspace {}'s Instance", a.workspace),
                given(&["instance", "release", &a.workspace]),
                Some(a.organization.clone()),
            )
        },
        A::CorrectFieldAction(a) => corrected(a, invocation),
        A::SignInAction(a) => {
            let mut says = match &a.harness {
                Some(harness) => format!("sign in to {harness}"),
                None => "sign in".to_owned(),
            };
            if let Some(method) = &a.method {
                says.push_str(&format!(" by {method}"));
            }
            if let Some(sign_in) = &a.sign_in {
                says.push_str(&format!(" and hold it in {sign_in}"));
            }
            Step::said(says)
        }
        A::InspectHarnessImageAction(a) => Step::said(format!(
            "check that {} provides {}{}",
            a.image.as_deref().unwrap_or("the image"),
            a.command.as_deref().map_or_else(
                || "the harness".to_owned(),
                |command| format!("`{command}`")
            ),
            a.harness
                .as_deref()
                .map_or_else(String::new, |harness| format!(" for {harness}")),
        )),
        A::CheckConnectionAction(a) => {
            let mut says = format!(
                "check that the {} at {} is running, or point --control-plane or \
                 KESTREL_CONTROL_PLANE at the one that is",
                a.service, invocation.control_plane
            );
            if a.compose {
                says.push_str("; `docker compose ps` shows whether it is up");
            }
            Step::said(says)
        }
        A::RetryReadAction(a) => {
            let says = match a.retry_after_seconds {
                Some(seconds) => format!("read again after {seconds} seconds"),
                None => "read again".to_owned(),
            };
            if wrote {
                return Step::said(says);
            }
            Step {
                repeated: true,
                ..Step::run(
                    says,
                    invocation
                        .args
                        .iter()
                        .map(|arg| Word::Given(arg.clone()))
                        .collect(),
                    None,
                )
            }
        }
        A::InspectOperationAction(a) if a.uncertain => Step::said(format!(
            "{} may already have taken effect; look before running it again",
            a.operation
        )),
        A::InspectOperationAction(_) => {
            let mut command: Vec<Word> = invocation
                .operation
                .split(' ')
                .map(|word| Word::Given(word.to_owned()))
                .collect();
            command.push(Word::Given("--help".to_owned()));
            Step::run(
                format!("see what `{BINARY} {}` accepts", invocation.operation),
                command,
                None,
            )
        }
    }
}

/// The invocation again with only the refused value asked for, so the rest of it is kept.
fn corrected(action: &wire::CorrectFieldAction, invocation: &Invocation) -> Step {
    let mut says = format!("correct {}: {}", action.field, action.constraint);
    if let Some(allowed) = &action.allowed_values {
        says.push_str(&format!("; one of {}", allowed.join(", ")));
    }
    let Some(flag) = flag_for(&action.field) else {
        return Step::said(says);
    };
    let mut command = Vec::new();
    let mut found = false;
    let mut args = invocation.args.iter();
    while let Some(arg) = args.next() {
        if *arg == flag {
            args.next();
        } else if !arg.starts_with(&format!("{flag}=")) {
            command.push(Word::Given(arg.clone()));
            continue;
        }
        if !found {
            command.push(Word::Asked(Input::named(&action.field)));
            found = true;
        }
    }
    if !found {
        return Step::said(says);
    }
    Step {
        repeated: true,
        ..Step::run(says, command, None)
    }
}

fn inspection(
    resource: &wire::Resource,
    reference: Option<&str>,
    organization: Option<&str>,
) -> (Vec<Word>, Option<String>) {
    use wire::Resource as R;
    let Some(reference) = reference else {
        return listing(resource, organization);
    };
    let scoped = organization.map(str::to_owned);
    match resource {
        R::Organization => (given(&["status"]), Some(reference.to_owned())),
        R::Workspace => (given(&["workspace", "show", reference]), scoped),
        R::Session => (given(&["session", "show", reference]), scoped),
        R::Trigger => (given(&["trigger", "show", reference]), scoped),
        R::Integration => (given(&["integration", "show", reference]), scoped),
        R::Event => (given(&["event", "show", reference]), None),
        resource => listing(resource, organization),
    }
}

fn listing(resource: &wire::Resource, organization: Option<&str>) -> (Vec<Word>, Option<String>) {
    use wire::Resource as R;
    let scoped = organization.map(str::to_owned);
    let command = match resource {
        R::Organization => return (given(&["organization", "list"]), None),
        R::Project => "project",
        R::Agent => "agent",
        R::SubscriptionProfile => "profile",
        R::ProviderCredential => "credential",
        R::Integration => "integration",
        R::Trigger => "trigger",
        R::Event => "event",
        R::Instance => "instance",
        R::Workspace | R::Session | R::HeldMessage | R::TranscriptPayload => "workspace",
    };
    (given(&[command, "list"]), scoped)
}

fn noun(resource: &wire::Resource) -> &'static str {
    use wire::Resource as R;
    match resource {
        R::Organization => "Organization",
        R::Project => "Project",
        R::Agent => "Agent",
        R::SubscriptionProfile => "Subscription Profile",
        R::ProviderCredential => "Provider Credential",
        R::Integration => "Integration",
        R::Trigger => "Trigger",
        R::Event => "Event",
        R::Workspace => "Workspace",
        R::Session => "Session",
        R::Instance => "Instance",
        R::HeldMessage => "Held Message",
        R::TranscriptPayload => "Transcript payload",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::tests::{diagnostic, invocation, missing_project, rendered, stopping};
    use super::*;

    #[test]
    fn missing_inputs_are_named_as_flags_and_never_guessed() {
        let said = rendered(&missing_project(), &invocation(), false);

        assert!(
            said.contains(
                "kestrel project declare absent --repository <REPOSITORY> --branch <BRANCH> \
                 --organization 'Acme East'"
            ),
            "{said}"
        );
        assert!(said.contains("needs --repository and --branch"), "{said}");
        assert!(!said.contains("git"), "{said}");
    }

    #[test]
    fn a_chosen_control_plane_is_kept() {
        let invocation = Invocation {
            control_plane: "http://10.0.0.2:7718".to_owned(),
            elsewhere: true,
            ..invocation()
        };

        let said = rendered(&missing_project(), &invocation, false);

        assert!(
            said.contains("--organization 'Acme East' --control-plane http://10.0.0.2:7718"),
            "{said}"
        );
    }

    #[test]
    fn a_destructive_step_shows_its_consequence() {
        let said = rendered(&stopping(), &invocation(), false);

        assert!(
            said.contains("kestrel session stop s --organization acme"),
            "{said}"
        );
        assert!(
            said.contains(
                "Stopping the session now records it failed. It happens only if you choose it."
            ),
            "{said}"
        );
    }

    #[test]
    fn several_candidates_are_each_offered_for_inspection() {
        let ambiguous = diagnostic(json!({
            "kind": "ambiguous_reference",
            "message": "ab matches more than one session",
            "field": null,
            "context": {
                "resource": "session",
                "reference": "ab",
                "organization": "acme",
                "candidates": [
                    { "id": "ab12", "name": "brisk-otter" },
                    { "id": "ab34", "name": "calm-heron" },
                ],
            },
            "next_steps": [{ "action": "list_resources", "resource": "session", "organization": "acme" }],
        }));

        let said = rendered(&ambiguous, &invocation(), false);

        assert!(
            said.contains("kestrel session show ab12 --organization acme"),
            "{said}"
        );
        assert!(
            said.contains("kestrel session show ab34 --organization acme"),
            "{said}"
        );
        assert!(said.contains("calm-heron"), "{said}");
    }

    #[test]
    fn a_corrected_field_keeps_the_rest_of_the_invocation() {
        let invalid = diagnostic(json!({
            "kind": "invalid_field",
            "message": "agent must name a declared agent",
            "field": "agent",
            "context": { "field": "agent", "constraint": "matches_declared", "allowed_values": null },
            "next_steps": [{
                "action": "correct_field",
                "operation": "open_workspace",
                "field": "agent",
                "constraint": "matches_declared",
                "allowed_values": null,
                "resource": null,
            }],
        }));

        let said = rendered(&invalid, &invocation(), false);

        assert!(
            said.contains("kestrel workspace open --project absent --agent <AGENT>\n"),
            "{said}"
        );
    }
}
