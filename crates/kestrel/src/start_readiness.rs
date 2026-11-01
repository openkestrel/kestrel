//! Reading whether work can start writes nothing, asks no provider, starts no relay and never
//! pulls an image (ADR-0052).

use std::collections::BTreeSet;

use crate::capability::{self, Capability, Images};
use crate::catalogue;
use crate::declined::{Candidate, Constraint, Reason, Resource};
use crate::domain::{Operator, Organization, Project, SubscriptionProfile};
use crate::integration;
use crate::sign_in::{self, Holding, SignIn, State, UseResult, handoff};
use crate::store::{Store, Tx};
use anyhow::Result;
use serde::Deserialize;

pub const OPERATION: &str = "show_readiness";

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Context {
    pub organization: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub sign_in_method: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ready,
    Blocked,
    Unavailable,
}

#[derive(Debug)]
pub enum Gap {
    /// What the producer a start goes through would refuse it with.
    Refused(Reason),
    NoSignIn {
        harness: Option<String>,
        method: Option<String>,
        /// The inspected image whose harnesses were the only ones looked at.
        image: Option<String>,
    },
    NeedsAttention(Attention),
    NoRepository,
    NoProject,
}

#[derive(Debug)]
pub enum Warning {
    Unchecked(SignIn),
    NeedsAttention(Attention),
    NotLending(Reason),
}

#[derive(Debug, Clone)]
pub struct Attention {
    pub sign_in: SignIn,
    pub state: State,
    /// Set when the evidence is about this model alone.
    pub model: Option<String>,
}

#[derive(Debug, Default)]
pub struct Selected {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub sign_in_method: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug)]
pub struct Readiness {
    pub organization: Option<Organization>,
    pub operator: Option<Operator>,
    pub initial_setup_complete: bool,
    pub selected: Selected,
    pub sign_in: Option<SignIn>,
    pub sign_ins: Vec<SignIn>,
    pub github_integrations: Vec<String>,
    pub projects: Vec<String>,
    pub gaps: Vec<Gap>,
    pub warnings: Vec<Warning>,
    /// Never a gap: a failed inspection establishes nothing about the harness.
    pub uninspectable: Option<Reason>,
}

impl Readiness {
    pub fn outcome(&self) -> Outcome {
        if !self.gaps.is_empty() {
            Outcome::Blocked
        } else if self.uninspectable.is_some() {
            Outcome::Unavailable
        } else {
            Outcome::Ready
        }
    }
}

pub async fn read(store: &Store, images: &Images, context: Context) -> Result<Readiness> {
    let context = Context {
        organization: given(context.organization),
        harness: given(context.harness),
        model: given(context.model),
        profile: given(context.profile),
        sign_in_method: given(context.sign_in_method),
        project: given(context.project),
    };
    offered(&context)?;
    // Before the transaction, so a slow daemon holds no read open.
    let capability = images.read().await;

    let mut tx = store.read().await?;
    let organization = scope(&mut tx, context.organization.as_deref()).await?;
    assessed(&mut tx, &capability, organization, &context).await
}

/// Readiness of a path within an Organization already chosen, or none yet declared.
pub(crate) async fn assessed(
    tx: &mut Tx<'_>,
    capability: &Capability,
    organization: Option<Organization>,
    context: &Context,
) -> Result<Readiness> {
    let operator = tx.operators().current().await?;
    let initial_setup_complete = tx.operators().initial_setup_complete().await?;

    let mut gaps = Vec::new();
    let mut warnings = Vec::new();
    if operator.is_none() {
        gaps.push(Gap::Refused(Reason::MissingOperator {
            operation: OPERATION,
        }));
    }

    let mut naming = Naming::OperatorsOwn;
    let mut sign_ins = Vec::new();
    let mut projects = Vec::new();
    let mut lending = Vec::new();
    let mut paused = Vec::new();
    if let Some(organization) = &organization {
        let mut holding = sign_in::operator_profiles(tx, organization).await?;
        if let Some(named) = &context.profile {
            match refusal(tx.profiles().named(organization, named).await)? {
                Ok(profile) => {
                    if !holding.iter().any(|held| held.id == profile.id) {
                        holding.push(profile.clone());
                    }
                    naming = Naming::Named(profile);
                }
                Err(missing) => {
                    gaps.push(Gap::Refused(missing));
                    naming = Naming::Missing;
                }
            }
        }
        sign_ins = sign_in::saved_in(tx, organization, holding).await?;
        projects = tx.projects().all(organization).await?;
        for integration in tx.integrations().all(organization).await? {
            match refusal(
                integration::lending(&integration, &organization.name, OPERATION).map(|_| ()),
            )? {
                Ok(()) => lending.push(integration.name),
                Err(refused @ Reason::StateConflict { .. }) if !integration.retired() => {
                    paused.push(Warning::NotLending(refused));
                }
                Err(_) => {}
            }
        }
    }

    let paths = Paths {
        capability,
        sign_ins: &sign_ins,
        naming: &naming,
        context,
        organization: organization.as_ref(),
    }
    .evaluated();
    gaps.extend(paths.gaps);
    warnings.extend(paths.warnings);

    let project = match (&context.project, &organization) {
        (Some(named), Some(organization)) => {
            match refusal(tx.projects().named(organization, named).await)? {
                Ok(project) => Some(project),
                Err(missing) => {
                    gaps.push(Gap::Refused(missing));
                    None
                }
            }
        }
        _ if projects.is_empty() => {
            if lending.is_empty() {
                gaps.push(Gap::NoRepository);
                warnings.extend(paused);
            } else {
                gaps.push(Gap::NoProject);
            }
            None
        }
        _ => (projects.len() == 1).then(|| projects[0].clone()),
    };
    if let (Some(project), Some(organization)) = (&project, &organization) {
        warnings.extend(not_lending(tx, organization, project).await?);
    }

    Ok(Readiness {
        selected: Selected {
            harness: context.harness.clone().or(paths.harness),
            model: context.model.clone(),
            profile: context.profile.clone().or_else(|| {
                paths.sign_in.as_ref().and_then(|one| match &one.holding {
                    Holding::Profile(profile) => Some(profile.name.clone()),
                    Holding::ProviderCredential(_) => None,
                })
            }),
            sign_in_method: context
                .sign_in_method
                .clone()
                .or_else(|| paths.sign_in.as_ref().map(|one| one.method.id.clone())),
            project: context
                .project
                .clone()
                .or_else(|| project.map(|project| project.name)),
        },
        sign_in: paths.sign_in,
        sign_ins,
        github_integrations: lending,
        projects: projects.into_iter().map(|project| project.name).collect(),
        gaps,
        warnings,
        uninspectable: uninspectable(capability, context.harness.as_deref()),
        organization,
        operator,
        initial_setup_complete,
    })
}

fn given(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

pub(crate) fn offered(context: &Context) -> Result<()> {
    let Some(method) = &context.sign_in_method else {
        return Ok(());
    };
    if let Some(harness) = &context.harness {
        catalogue::sign_in_method(OPERATION, harness, method)
            .map_err(|error| handoff::concerning_the_method(error, harness))?;
        return Ok(());
    }

    let methods: BTreeSet<&str> = catalogue::harnesses()
        .iter()
        .flat_map(|row| &row.sign_in_methods)
        .map(|offered| offered.id.as_str())
        .collect();
    if methods.contains(method.as_str()) {
        return Ok(());
    }
    Err(Reason::InvalidField {
        field: "sign_in_method",
        operation: OPERATION,
        constraint: Constraint::Offered,
        allowed: Some(methods.into_iter().map(str::to_owned).collect()),
        message: format!("no harness offers a Sign-in Method {method}"),
    }
    .into())
}

/// The one named, or the only one; several and none named is never a choice made here.
async fn scope(tx: &mut Tx<'_>, named: Option<&str>) -> Result<Option<Organization>> {
    if let Some(named) = named {
        return tx.organizations().named(named).await.map(Some);
    }
    let mut all = tx.organizations().all().await?;
    if all.len() > 1 {
        return Err(Reason::AmbiguousReference {
            resource: Resource::Organization,
            reference: String::new(),
            organization: None,
            message: format!(
                "{} Organizations exist and none was named: {}",
                all.len(),
                all.iter()
                    .map(|organization| organization.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            candidates: all
                .into_iter()
                .map(|organization| Candidate {
                    id: organization.id.to_string(),
                    name: organization.name,
                })
                .collect(),
        }
        .into());
    }

    Ok(all.pop())
}

/// A typed refusal is a gap to report; anything else is a read that failed.
fn refusal<T>(read: Result<T>) -> Result<Result<T, Reason>> {
    match read {
        Ok(found) => Ok(Ok(found)),
        Err(error) => error.downcast::<Reason>().map(Err),
    }
}

async fn not_lending(
    tx: &mut Tx<'_>,
    organization: &Organization,
    project: &Project,
) -> Result<Vec<Warning>> {
    let mut warnings = Vec::new();
    let mut seen = BTreeSet::new();
    for selected in &project.integrations {
        if !seen.insert(selected.integration.to_string()) {
            continue;
        }
        let Some(integration) = tx.integrations().find(selected.integration).await? else {
            continue;
        };
        if let Err(refused) =
            refusal(integration::lending(&integration, &organization.name, OPERATION).map(|_| ()))?
        {
            warnings.push(Warning::NotLending(refused));
        }
    }

    Ok(warnings)
}

fn uninspectable(capability: &Capability, harness: Option<&str>) -> Option<Reason> {
    let Capability::Unavailable { image, cause } = capability else {
        return None;
    };
    Some(Reason::ImageUnavailable {
        operation: OPERATION,
        harness: harness.map(str::to_owned),
        image: image.clone(),
        message: capability::unavailable(image, cause, harness),
    })
}

enum Naming {
    OperatorsOwn,
    Named(SubscriptionProfile),
    Missing,
}

struct Paths<'a> {
    capability: &'a Capability,
    sign_ins: &'a [SignIn],
    naming: &'a Naming,
    context: &'a Context,
    organization: Option<&'a Organization>,
}

#[derive(Default)]
struct Evaluated {
    gaps: Vec<Gap>,
    warnings: Vec<Warning>,
    harness: Option<String>,
    sign_in: Option<SignIn>,
}

impl Paths<'_> {
    fn evaluated(&self) -> Evaluated {
        let mut evaluated = Evaluated::default();
        let model = self.context.model.as_deref();

        let chosen: Vec<&SignIn> = match self.context.harness.as_deref() {
            Some(harness) => {
                if let Err(not_carried) = self.capability.clone().admits(harness, OPERATION)
                    && matches!(*not_carried, Reason::HarnessNotCarried { .. })
                {
                    evaluated.gaps.push(Gap::Refused(*not_carried));
                    return self.warned_of_the_rest(evaluated, &[]);
                }
                if !catalogue::harnesses().iter().any(|row| row.name == harness) {
                    // No guided method exists to find saved or missing.
                    return self.warned_of_the_rest(evaluated, &[]);
                }
                let (chosen, ambiguous) = self.chosen(harness, true);
                if let Some(ambiguous) = ambiguous {
                    evaluated.gaps.push(ambiguous);
                    return self.warned_of_the_rest(evaluated, &[]);
                }
                chosen
            }
            None => {
                let mut chosen = Vec::new();
                let mut ambiguous = None;
                for row in catalogue::harnesses() {
                    if !self.carried(&row.name) {
                        continue;
                    }
                    let (found, among_several) = self.chosen(&row.name, false);
                    chosen.extend(found);
                    ambiguous = ambiguous.or(among_several);
                }
                if let (true, Some(ambiguous)) = (chosen.is_empty(), ambiguous) {
                    evaluated.gaps.push(ambiguous);
                    return self.warned_of_the_rest(evaluated, &[]);
                }
                chosen
            }
        };

        if chosen.is_empty() {
            if !matches!(self.naming, Naming::Missing) {
                evaluated.gaps.push(Gap::NoSignIn {
                    harness: self.context.harness.clone(),
                    method: self.context.sign_in_method.clone(),
                    image: match (self.capability, &self.context.harness) {
                        (Capability::Inspected { image, .. }, None) => Some(image.clone()),
                        _ => None,
                    },
                });
            }
            return self.warned_of_the_rest(evaluated, &[]);
        }

        let usable: Vec<&SignIn> = chosen
            .iter()
            .copied()
            .filter(|one| attention(one, model).is_none())
            .collect();
        let mut reported = Vec::new();
        if usable.is_empty() {
            for one in &chosen {
                if reported.contains(&one.revision) {
                    continue;
                }
                reported.push(one.revision);
                evaluated
                    .gaps
                    .extend(attention(one, model).map(Gap::NeedsAttention));
            }
            return self.warned_of_the_rest(evaluated, &reported);
        }

        let harnesses: BTreeSet<&str> = usable.iter().map(|one| one.harness.as_str()).collect();
        if harnesses.len() == 1 {
            evaluated.harness = harnesses.first().map(|harness| (*harness).to_owned());
        }
        if let [only] = usable.as_slice() {
            evaluated.sign_in = Some((*only).clone());
        }
        if usable
            .iter()
            .all(|one| one.authentication.state == State::Unchecked)
        {
            for one in usable {
                if !reported.contains(&one.revision) {
                    reported.push(one.revision);
                    evaluated.warnings.push(Warning::Unchecked(one.clone()));
                }
            }
        }

        self.warned_of_the_rest(evaluated, &reported)
    }

    /// Broken material the path does not depend on never blocks it.
    fn warned_of_the_rest(&self, mut evaluated: Evaluated, reported: &[i64]) -> Evaluated {
        let mut reported = reported.to_vec();
        for one in self.sign_ins {
            if reported.contains(&one.revision) {
                continue;
            }
            if let Some(attention) = attention(one, self.context.model.as_deref()) {
                reported.push(one.revision);
                evaluated.warnings.push(Warning::NeedsAttention(attention));
            }
        }
        evaluated
    }

    fn carried(&self, harness: &str) -> bool {
        match self.capability {
            Capability::Inspected { harnesses, .. } => harnesses.contains(harness),
            Capability::Unchecked | Capability::Unavailable { .. } => true,
        }
    }

    /// `by_default` is what an open naming no method takes (ADR-0049), not every alternative.
    fn chosen(&self, harness: &str, by_default: bool) -> (Vec<&SignIn>, Option<Gap>) {
        let method = self.context.sign_in_method.as_deref();
        let of = || {
            self.sign_ins
                .iter()
                .filter(move |one| one.harness == harness)
                .filter(move |one| method.is_none_or(|method| one.method.id == method))
        };
        let keys = of().filter(|one| matches!(one.holding, Holding::ProviderCredential(_)));
        let subscriptions: Vec<&SignIn> = of()
            .filter(|one| match (&one.holding, self.naming) {
                (Holding::Profile(profile), Naming::Named(named)) => profile.id == named.id,
                (Holding::Profile(profile), Naming::OperatorsOwn) => {
                    profile.owner_operator.is_some()
                }
                _ => false,
            })
            .collect();

        if !matches!(self.naming, Naming::OperatorsOwn) {
            return (keys.chain(subscriptions).collect(), None);
        }

        let mut holders: Vec<&SubscriptionProfile> = Vec::new();
        for one in &subscriptions {
            if let Holding::Profile(profile) = &one.holding
                && !holders.iter().any(|held| held.id == profile.id)
            {
                holders.push(profile);
            }
        }
        if let (true, Some(organization)) = (holders.len() > 1, self.organization) {
            let among: Vec<SubscriptionProfile> = holders.into_iter().cloned().collect();
            let message = format!(
                "the Operator owns several Subscription Profiles in {} holding a {harness} \
                 subscription ({}); name the one this work uses",
                organization.name,
                sign_in::names(&among)
            );
            let ambiguous = Gap::Refused(sign_in::several_operator_profiles(
                OPERATION,
                organization,
                &among,
                message,
            ));
            return (
                if by_default {
                    Vec::new()
                } else {
                    keys.collect()
                },
                Some(ambiguous),
            );
        }

        let methods: BTreeSet<&str> = subscriptions
            .iter()
            .map(|one| one.method.id.as_str())
            .collect();
        // The Operator's one subscription is selected, so the Organization's key is withheld.
        if by_default && method.is_none() && methods.len() == 1 {
            return (subscriptions, None);
        }

        (keys.chain(subscriptions).collect(), None)
    }
}

/// A model's result is evidence about that model only, and none is no evidence against it.
fn attention(sign_in: &SignIn, model: Option<&str>) -> Option<Attention> {
    let state = match sign_in.authentication.state {
        state @ (State::AuthenticationFailed | State::Expired | State::NotCovered) => {
            Some((state, None))
        }
        State::Unchecked | State::LoginCompleted | State::CredentialAccepted => model
            .and_then(|model| sign_in.model_use.iter().find(|used| used.model == model))
            .and_then(|used| match used.result {
                UseResult::Worked => None,
                UseResult::AuthenticationFailed => Some(State::AuthenticationFailed),
                UseResult::NotCovered => Some(State::NotCovered),
            })
            .map(|state| (state, model.map(str::to_owned))),
    };

    state.map(|(state, model)| Attention {
        sign_in: sign_in.clone(),
        state,
        model,
    })
}

impl Attention {
    pub fn message(&self) -> String {
        let sign_in = &self.sign_in;
        let held = match &sign_in.holding {
            Holding::Profile(profile) => format!("the subscription profile {}", profile.name),
            Holding::ProviderCredential(variable) => {
                format!("the provider credential {variable}")
            }
        };
        let said = format!(
            "{}, the {} sign-in held in {held}",
            sign_in.method.name, sign_in.harness
        );
        match (self.state, &self.model) {
            (State::Expired, _) => format!("{said}, has expired"),
            (State::NotCovered, Some(model)) => {
                format!("{said}, does not cover the model {model}")
            }
            (State::NotCovered, None) => format!("{said}, does not cover its harness"),
            (_, Some(model)) => format!(
                "{said}, needs attention: the harness reported authentication failed on the \
                 model {model}"
            ),
            _ => {
                format!("{said}, needs attention: the harness reported authentication failed")
            }
        }
    }
}

impl Gap {
    pub fn message(&self, organization: Option<&str>) -> String {
        let within = organization.map_or_else(String::new, |organization| {
            format!(" in the organization {organization}")
        });
        match self {
            Gap::Refused(reason) => reason.to_string(),
            Gap::NoSignIn {
                harness: Some(harness),
                method: Some(method),
                ..
            } => format!("no {method} sign-in for the harness {harness} is saved{within}"),
            Gap::NoSignIn {
                harness: Some(harness),
                method: None,
                ..
            } => format!("the harness {harness} is not signed in{within}"),
            Gap::NoSignIn {
                harness: None,
                method: Some(method),
                ..
            } => format!("no {method} sign-in is saved{within}"),
            Gap::NoSignIn {
                harness: None,
                method: None,
                image: Some(image),
            } => format!("no harness the image {image} carries is signed in{within}"),
            Gap::NoSignIn {
                harness: None,
                method: None,
                image: None,
            } => format!("no harness is signed in{within}"),
            Gap::NeedsAttention(attention) => attention.message(),
            Gap::NoRepository => format!(
                "no repository is available{within}: it declares no project, and no github \
                 integration reaches one"
            ),
            Gap::NoProject => {
                format!("no project is declared{within}; choose a repository for the first one")
            }
        }
    }
}

pub fn unchecked(sign_in: &SignIn) -> String {
    format!(
        "{}, the {} sign-in this rests on, has not been checked",
        sign_in.method.name, sign_in.harness
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::{OperatorId, OrganizationId, SubscriptionProfileId};
    use crate::sign_in::{Authentication, ModelUse, Source, UseSource};

    fn organization() -> Organization {
        Organization {
            id: OrganizationId::generate(),
            name: "acme".to_owned(),
            max_live_instances: None,
        }
    }

    fn profile(name: &str) -> SubscriptionProfile {
        SubscriptionProfile {
            id: SubscriptionProfileId::generate(),
            organization: OrganizationId::generate(),
            name: name.to_owned(),
            owner: "ada".to_owned(),
            owner_operator: Some(OperatorId::generate()),
        }
    }

    fn saved(harness: &str, method: &str, holding: Holding, revision: i64, state: State) -> SignIn {
        SignIn {
            harness: harness.to_owned(),
            method: catalogue::sign_in_method("test", harness, method).expect("a method"),
            holding,
            revision,
            refreshed_from: None,
            saved_at: crate::timer::Clock::System.now(),
            authentication: Authentication {
                state,
                source: Source::Import,
                observed_at: crate::timer::Clock::System.now(),
                provider_check: None,
            },
            model_use: Vec::new(),
        }
    }

    fn key(harness: &str, method: &str, variable: &str, revision: i64, state: State) -> SignIn {
        saved(
            harness,
            method,
            Holding::ProviderCredential(variable.to_owned()),
            revision,
            state,
        )
    }

    fn carrying(names: &[&str]) -> Capability {
        Capability::Inspected {
            image: "kestrel-env".to_owned(),
            identity: "sha256:one".to_owned(),
            harnesses: Arc::new(names.iter().map(|name| (*name).to_owned()).collect()),
        }
    }

    fn evaluated(capability: &Capability, sign_ins: &[SignIn], context: &Context) -> Evaluated {
        let organization = organization();
        Paths {
            capability,
            sign_ins,
            naming: &Naming::OperatorsOwn,
            context,
            organization: Some(&organization),
        }
        .evaluated()
    }

    fn on(harness: &str) -> Context {
        Context {
            harness: Some(harness.to_owned()),
            ..Context::default()
        }
    }

    #[test]
    fn a_sign_in_for_a_harness_the_image_does_not_carry_is_no_usable_path() {
        let sign_ins = [key(
            "codex",
            "openai-api-key",
            "OPENAI_API_KEY",
            1,
            State::CredentialAccepted,
        )];

        let found = evaluated(&carrying(&["claude"]), &sign_ins, &Context::default());

        assert!(
            matches!(
                found.gaps.as_slice(),
                [Gap::NoSignIn {
                    harness: None,
                    method: None,
                    image: Some(_),
                }]
            ),
            "{:?}",
            found.gaps
        );
    }

    #[test]
    fn a_named_harness_the_image_does_not_carry_blocks_though_another_is_usable() {
        let sign_ins = [
            key(
                "claude",
                "anthropic-api-key",
                "ANTHROPIC_API_KEY",
                1,
                State::CredentialAccepted,
            ),
            key(
                "opencode",
                "anthropic-api-key",
                "ANTHROPIC_API_KEY",
                1,
                State::CredentialAccepted,
            ),
        ];
        let image = carrying(&["opencode"]);

        let generic = evaluated(&image, &sign_ins, &Context::default());
        assert!(generic.gaps.is_empty(), "{:?}", generic.gaps);
        assert_eq!(generic.harness.as_deref(), Some("opencode"));

        let named = evaluated(&image, &sign_ins, &on("claude"));
        assert!(
            matches!(
                named.gaps.as_slice(),
                [Gap::Refused(Reason::HarnessNotCarried { harness, .. })] if harness == "claude"
            ),
            "{:?}",
            named.gaps
        );
    }

    #[test]
    fn an_uninspectable_image_never_makes_a_harness_absent() {
        let sign_ins = [key(
            "claude",
            "anthropic-api-key",
            "ANTHROPIC_API_KEY",
            1,
            State::CredentialAccepted,
        )];
        let down = Capability::Unavailable {
            image: "kestrel-env".to_owned(),
            cause: capability::Uninspectable::Failed("down".to_owned()),
        };

        let found = evaluated(&down, &sign_ins, &on("claude"));

        assert!(found.gaps.is_empty(), "{:?}", found.gaps);
        assert!(uninspectable(&down, Some("claude")).is_some());
    }

    #[test]
    fn a_named_harness_takes_the_operators_subscription_and_withholds_the_key() {
        let jack = profile("jack");
        let sign_ins = [
            saved(
                "claude",
                "claude-setup-token",
                Holding::Profile(jack),
                1,
                State::AuthenticationFailed,
            ),
            key(
                "claude",
                "anthropic-api-key",
                "ANTHROPIC_API_KEY",
                2,
                State::CredentialAccepted,
            ),
        ];

        let named = evaluated(&Capability::Unchecked, &sign_ins, &on("claude"));
        assert!(
            matches!(named.gaps.as_slice(), [Gap::NeedsAttention(attention)] if attention.sign_in.revision == 1),
            "{:?}",
            named.gaps
        );

        let generic = evaluated(&Capability::Unchecked, &sign_ins, &Context::default());
        assert!(generic.gaps.is_empty(), "{:?}", generic.gaps);
        assert_eq!(generic.sign_in.map(|one| one.revision), Some(2));
        assert!(
            matches!(generic.warnings.as_slice(), [Warning::NeedsAttention(attention)] if attention.sign_in.revision == 1),
            "{:?}",
            generic.warnings
        );
    }

    #[test]
    fn several_of_the_operators_profiles_holding_a_subscription_choose_none() {
        let sign_ins = [
            saved(
                "claude",
                "claude-setup-token",
                Holding::Profile(profile("work")),
                1,
                State::LoginCompleted,
            ),
            saved(
                "claude",
                "claude-setup-token",
                Holding::Profile(profile("home")),
                2,
                State::LoginCompleted,
            ),
        ];

        let found = evaluated(&Capability::Unchecked, &sign_ins, &on("claude"));

        assert!(
            matches!(
                found.gaps.as_slice(),
                [Gap::Refused(Reason::StateConflict { state, .. })] if *state == "several_operator_profiles"
            ),
            "{:?}",
            found.gaps
        );
        assert!(found.sign_in.is_none());
    }

    #[test]
    fn one_models_result_never_stands_for_another() {
        let mut held = key(
            "claude",
            "anthropic-api-key",
            "ANTHROPIC_API_KEY",
            1,
            State::CredentialAccepted,
        );
        held.model_use.push(ModelUse {
            harness: "claude".to_owned(),
            model: "opus".to_owned(),
            image: None,
            result: UseResult::NotCovered,
            source: UseSource::Session,
            observed_at: crate::timer::Clock::System.now(),
        });
        let sign_ins = [held];
        let with = |model: Option<&str>| Context {
            model: model.map(str::to_owned),
            ..on("claude")
        };

        let covered = evaluated(&Capability::Unchecked, &sign_ins, &with(Some("sonnet")));
        assert!(covered.gaps.is_empty(), "{:?}", covered.gaps);
        let unnamed = evaluated(&Capability::Unchecked, &sign_ins, &with(None));
        assert!(unnamed.gaps.is_empty(), "{:?}", unnamed.gaps);

        let refused = evaluated(&Capability::Unchecked, &sign_ins, &with(Some("opus")));
        assert!(
            matches!(
                refused.gaps.as_slice(),
                [Gap::NeedsAttention(Attention { state: State::NotCovered, model: Some(model), .. })] if model == "opus"
            ),
            "{:?}",
            refused.gaps
        );
    }
}
