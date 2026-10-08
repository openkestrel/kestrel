// Allow only the lint patterns emitted by the pinned generator.
#[allow(
    unused_variables,
    clippy::double_must_use,
    clippy::redundant_field_names,
    clippy::collapsible_if,
    clippy::nonminimal_bool
)]
mod generated;
mod sse;
pub use generated::*;
