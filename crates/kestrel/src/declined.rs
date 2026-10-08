use std::fmt;

/// Why kestrel would not do what it was asked, told apart so a boundary can answer each
/// differently. Its `Display` is the reason alone.
#[derive(Debug)]
pub enum Declined {
    Unacceptable(String),
    Missing(String),
    Ambiguous(String),
    Taken(String),
}

impl fmt::Display for Declined {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Declined::Unacceptable(why)
            | Declined::Missing(why)
            | Declined::Ambiguous(why)
            | Declined::Taken(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Declined {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Organization,
    Project,
    Agent,
    SubscriptionProfile,
    ProviderCredential,
    Integration,
    Trigger,
    Event,
    Workspace,
    Session,
    Instance,
    HeldMessage,
    TranscriptPayload,
}

impl Resource {
    pub const fn noun(self) -> &'static str {
        match self {
            Resource::Organization => "organization",
            Resource::Project => "project",
            Resource::Agent => "agent",
            Resource::SubscriptionProfile => "subscription profile",
            Resource::ProviderCredential => "provider credential",
            Resource::Integration => "integration",
            Resource::Trigger => "trigger",
            Resource::Event => "event",
            Resource::Workspace => "workspace",
            Resource::Session => "session",
            Resource::Instance => "instance",
            Resource::HeldMessage => "held message",
            Resource::TranscriptPayload => "transcript payload",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locator {
    pub resource: Resource,
    pub reference: String,
}

impl Locator {
    pub fn new(resource: Resource, reference: impl Into<String>) -> Self {
        Self {
            resource,
            reference: reference.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    NonEmpty,
    DistinctCheckoutDirectories,
    MatchesDeclared,
    NotReserved,
    EnvironmentVariableName,
    BeneathHome,
    MaxLength,
    NoControlCharacters,
    NotAnAgentName,
    OmittedWhenContinuing,
    RequiresBrief,
    Offered,
}

impl Constraint {
    pub const fn as_str(self) -> &'static str {
        match self {
            Constraint::NonEmpty => "non_empty",
            Constraint::DistinctCheckoutDirectories => "distinct_checkout_directories",
            Constraint::MatchesDeclared => "matches_declared",
            Constraint::NotReserved => "not_reserved",
            Constraint::EnvironmentVariableName => "environment_variable_name",
            Constraint::BeneathHome => "beneath_home",
            Constraint::MaxLength => "max_length",
            Constraint::NoControlCharacters => "no_control_characters",
            Constraint::NotAnAgentName => "not_an_agent_name",
            Constraint::OmittedWhenContinuing => "omitted_when_continuing",
            Constraint::RequiresBrief => "requires_brief",
            Constraint::Offered => "offered",
        }
    }
}

/// What a state-changing step would do to work, so a Client can show it beside the choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consequence {
    FailsSession,
    EndsSession,
    DiscardsUnpublishedWork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    StopSession {
        session: String,
        consequence: Consequence,
    },
    EnqueueSession {
        workspace: String,
    },
    ReleaseInstance {
        workspace: String,
        instance: String,
    },
}

/// Inspection comes first by construction; every other step is one a person chooses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Next {
    pub inspect: Locator,
    pub then: Vec<Step>,
}

impl Next {
    pub fn inspect(resource: Resource, reference: impl Into<String>) -> Self {
        Self {
            inspect: Locator::new(resource, reference),
            then: Vec::new(),
        }
    }

    #[must_use]
    pub fn then(mut self, step: Step) -> Self {
        self.then.push(step);
        self
    }
}

/// `message` is display only; the boundary maps the other fields without reading it (ADR-0052).
#[derive(Debug)]
pub enum Reason {
    MissingOperator {
        operation: &'static str,
    },
    MissingReference {
        resource: Resource,
        reference: String,
        organization: Option<String>,
        /// The record to inspect instead of listing, when the reference lives inside one.
        within: Option<Locator>,
        message: String,
    },
    AmbiguousReference {
        resource: Resource,
        reference: String,
        organization: Option<String>,
        candidates: Vec<Candidate>,
        message: String,
    },
    InvalidField {
        field: &'static str,
        operation: &'static str,
        constraint: Constraint,
        allowed: Option<Vec<String>>,
        message: String,
    },
    StateConflict {
        operation: &'static str,
        resource: Resource,
        reference: String,
        organization: Option<String>,
        state: &'static str,
        holding_session: Option<String>,
        next: Next,
        message: String,
    },
    Forbidden {
        operation: &'static str,
        resource: Resource,
        reference: String,
        organization: Option<String>,
        constraint: &'static str,
        next: Next,
        message: String,
    },
    Expired {
        operation: &'static str,
        resource: Resource,
        reference: String,
        organization: Option<String>,
        next: Next,
        message: String,
    },
    InstanceTimeout {
        operation: &'static str,
        workspace: String,
        instance: String,
        message: String,
    },
}

impl Reason {
    pub fn concerning(self, field: &'static str) -> Concerning {
        Concerning {
            field,
            reason: self,
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Reason::MissingOperator { .. } => "name the Operator before using their Profile",
            Reason::MissingReference { message, .. }
            | Reason::AmbiguousReference { message, .. }
            | Reason::InvalidField { message, .. }
            | Reason::StateConflict { message, .. }
            | Reason::Forbidden { message, .. }
            | Reason::Expired { message, .. }
            | Reason::InstanceTimeout { message, .. } => message,
        };
        f.write_str(message)
    }
}

impl std::error::Error for Reason {}

#[derive(Debug)]
pub struct Concerning {
    pub field: &'static str,
    pub reason: Reason,
}

impl fmt::Display for Concerning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(f)
    }
}

impl std::error::Error for Concerning {}
