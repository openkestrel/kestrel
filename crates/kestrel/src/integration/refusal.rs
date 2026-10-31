use crate::declined::{Next, Reason, Resource, Step};
use crate::domain::Integration;

pub fn revised(
    read: &Integration,
    organization: &str,
    operation: &'static str,
    against: i64,
    next: Next,
) -> Reason {
    Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: read.name.clone(),
        organization: Some(organization.to_owned()),
        state: "revised",
        holding_session: None,
        next,
        message: format!(
            "the integration {} has changed since revision {against}; read it again and \
             decide on what it is now",
            read.name
        ),
    }
}

/// A retired one names no repair: offering another Integration would choose its replacement.
pub fn out_of_use(
    integration: &Integration,
    organization: &str,
    operation: &'static str,
) -> Reason {
    let inspect = Next::inspect(Resource::Integration, integration.name.clone());
    let (next, message) = if integration.retired() {
        (
            inspect,
            format!(
                "the integration {} is retired: its credentials were erased, and it cannot be \
                 used, changed or enabled again",
                integration.name
            ),
        )
    } else {
        (
            inspect.then(Step::EnableIntegration {
                integration: integration.name.clone(),
            }),
            format!(
                "the integration {} is disabled; enable it to resume what waits on it",
                integration.name
            ),
        )
    };
    Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: integration.name.clone(),
        organization: Some(organization.to_owned()),
        state: integration.state.as_str(),
        holding_session: None,
        next,
        message,
    }
}
