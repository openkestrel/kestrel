//! One file per `kestrel` subcommand: its arguments and what it does with them.

use crate::api::ControlPlane;
use crate::diagnostic::Invocation;
use crate::output::Presentation;
use reqwest::Url;

pub(crate) mod agent;
pub(crate) mod credential;
pub(crate) mod event;
pub(crate) mod instance;
pub(crate) mod integration;
pub(crate) mod operator;
pub(crate) mod organization;
pub(crate) mod profile;
pub(crate) mod project;
pub(crate) mod session;
pub(crate) mod trigger;
pub(crate) mod workspace;

/// What every subcommand is run with, besides its own arguments and the scope it resolves.
#[derive(Clone, Copy)]
pub(crate) struct Running<'a> {
    pub(crate) api: &'a ControlPlane,
    pub(crate) presentation: &'a Presentation,
    pub(crate) json: bool,
    /// Parsed, for a command that hands the address on.
    pub(crate) control_plane: &'a Url,
    /// As the person gave it, for a message that repeats it.
    pub(crate) control_plane_given: &'a str,
    pub(crate) invocation: &'a Invocation,
}
