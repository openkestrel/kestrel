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

/// A value a person named that kestrel refuses, carrying the request field it came in so a Client
/// can say which one.
#[derive(Debug)]
pub struct FieldRefusal {
    pub field: &'static str,
    pub message: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Unacceptable,
    Missing,
    Ambiguous,
    Taken,
}

impl FieldRefusal {
    pub fn unacceptable(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
            kind: Kind::Unacceptable,
        }
    }
}

impl fmt::Display for FieldRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FieldRefusal {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Organization,
    Project,
    Agent,
    SubscriptionProfile,
    ProviderCredential,
    Workspace,
    Session,
}

impl Resource {
    pub const fn noun(self) -> &'static str {
        match self {
            Resource::Organization => "organization",
            Resource::Project => "project",
            Resource::Agent => "agent",
            Resource::SubscriptionProfile => "subscription profile",
            Resource::ProviderCredential => "provider credential",
            Resource::Workspace => "workspace",
            Resource::Session => "session",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    NonEmpty,
    DistinctCheckoutDirectories,
    MatchesDeclared,
    NotReserved,
    EnvironmentVariableName,
    BeneathHome,
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
        }
    }
}

/// `message` is display only; the boundary maps the other fields without reading it (ADR-0052).
#[derive(Debug)]
pub enum Reason {
    MissingReference {
        resource: Resource,
        reference: String,
        organization: Option<String>,
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
        message: String,
    },
    Taken {
        operation: &'static str,
        state: &'static str,
        resource: Resource,
        reference: String,
        organization: Option<String>,
        message: String,
    },
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Reason::MissingReference { message, .. }
            | Reason::AmbiguousReference { message, .. }
            | Reason::InvalidField { message, .. }
            | Reason::Taken { message, .. } => message,
        };
        f.write_str(message)
    }
}

impl std::error::Error for Reason {}
