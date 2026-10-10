//! Generated types from OpenAPI specification
//!
//! This file contains all the generated types for the API.
//! Do not edit manually - regenerate using the appropriate script.
#![allow(clippy::large_enum_variant)]
#![allow(clippy::format_in_format_args)]
#![allow(clippy::let_unit_value)]
#![allow(unreachable_patterns)]
use serde::{Deserialize, Serialize};
/// Serde normally maps both a missing `Option<T>` field and an
/// explicit JSON null to `None`. Wrapping the decoded value in
/// `Some` retains the field-presence bit for `Option<Option<T>>`.
mod tri_state_serde {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        T::deserialize(de).map(Some)
    }
}
#[derive(Debug, Clone)]
pub enum WorkspaceWork {
    WorkReported(WorkReported),
    WorkNoInstance(WorkNoInstance),
    WorkNotAnswering(WorkNotAnswering),
}
impl serde::Serialize for WorkspaceWork {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::WorkReported(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WorkReported),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("state") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "reported") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "state",
                            stringify!(WorkReported),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "state",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "state".to_string(),
                            serde_json::Value::String("reported".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::WorkNoInstance(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WorkNoInstance),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("state") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "no_instance") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "state",
                            stringify!(WorkNoInstance),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "state",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "state".to_string(),
                            serde_json::Value::String("no_instance".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::WorkNotAnswering(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WorkNotAnswering),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("state") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "not_answering") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "state",
                            stringify!(WorkNotAnswering),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "state",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "state".to_string(),
                            serde_json::Value::String("not_answering".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for WorkspaceWork {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("state") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "state",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "reported" => {
                    let primary_error = match serde_json::from_value::<WorkReported>(value.clone())
                    {
                        Ok(payload) => return Ok(Self::WorkReported(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<WorkNoInstance>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "state",
                                "reported",
                                first_name,
                                stringify!(WorkNoInstance),
                            )));
                        }
                        structural_match =
                            Some((Self::WorkNoInstance(payload), stringify!(WorkNoInstance)));
                    }
                    if let Ok(payload) = serde_json::from_value::<WorkNotAnswering>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "state",
                                "reported",
                                first_name,
                                stringify!(WorkNotAnswering),
                            )));
                        }
                        structural_match = Some((
                            Self::WorkNotAnswering(payload),
                            stringify!(WorkNotAnswering),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "no_instance" => {
                    let primary_error =
                        match serde_json::from_value::<WorkNoInstance>(value.clone()) {
                            Ok(payload) => return Ok(Self::WorkNoInstance(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<WorkReported>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "state",
                                "no_instance",
                                first_name,
                                stringify!(WorkReported),
                            )));
                        }
                        structural_match =
                            Some((Self::WorkReported(payload), stringify!(WorkReported)));
                    }
                    if let Ok(payload) = serde_json::from_value::<WorkNotAnswering>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "state",
                                "no_instance",
                                first_name,
                                stringify!(WorkNotAnswering),
                            )));
                        }
                        structural_match = Some((
                            Self::WorkNotAnswering(payload),
                            stringify!(WorkNotAnswering),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "not_answering" => {
                    let primary_error =
                        match serde_json::from_value::<WorkNotAnswering>(value.clone()) {
                            Ok(payload) => return Ok(Self::WorkNotAnswering(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<WorkReported>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "state",
                                "not_answering",
                                first_name,
                                stringify!(WorkReported),
                            )));
                        }
                        structural_match =
                            Some((Self::WorkReported(payload), stringify!(WorkReported)));
                    }
                    if let Ok(payload) = serde_json::from_value::<WorkNoInstance>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "state",
                                "not_answering",
                                first_name,
                                stringify!(WorkNoInstance),
                            )));
                        }
                        structural_match =
                            Some((Self::WorkNoInstance(payload), stringify!(WorkNoInstance)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "state",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "state",
                "`",
            ))),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceStashes {
    pub repositories: Vec<RepositoryText>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceCommits {
    pub repositories: Vec<RepositoryText>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryText {
    pub repository: String,
    pub text: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceChanges {
    pub repositories: Vec<RepositoryDiff>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkReported {
    ///The last report of each other Instance the Workspace has had, newest first, each history for its own Instance.
    pub earlier_reports: Vec<WorkInstanceReport>,
    pub last_report: WorkLastReport,
    pub reported_at: String,
    pub repositories: Vec<WorkRepository>,
    pub state: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkNotAnswering {
    ///The last report of each other Instance the Workspace has had, newest first, each history for its own Instance.
    pub earlier_reports: Vec<WorkInstanceReport>,
    pub last_report: WorkLastReport,
    pub message: String,
    pub state: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkNoInstance {
    pub branch: String,
    ///The last report of each other Instance the Workspace has had, newest first, each history for its own Instance.
    pub earlier_reports: Vec<WorkInstanceReport>,
    pub last_report: WorkLastReport,
    pub pull_request: Option<String>,
    pub state: serde_json::Value,
}
///The last complete work report any of the Workspace's Instances sent, kept across link loss, release and restart. It is history: it is no evidence of current Unpublished Work, authorizes no seal or release, and makes no Files or Changes read live.
#[derive(Debug, Clone)]
pub enum WorkLastReport {
    WorkNoReport(WorkNoReport),
    WorkInstanceReport(WorkInstanceReport),
}
impl serde::Serialize for WorkLastReport {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::WorkNoReport(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WorkNoReport),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("report") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "none") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "report",
                            stringify!(WorkNoReport),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "report",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "report".to_string(),
                            serde_json::Value::String("none".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::WorkInstanceReport(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WorkInstanceReport),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("report") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "received") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "report",
                            stringify!(WorkInstanceReport),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "report",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "report".to_string(),
                            serde_json::Value::String("received".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for WorkLastReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("report") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "report",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "none" => {
                    let primary_error = match serde_json::from_value::<WorkNoReport>(value.clone())
                    {
                        Ok(payload) => return Ok(Self::WorkNoReport(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<WorkInstanceReport>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "report",
                                "none",
                                first_name,
                                stringify!(WorkInstanceReport),
                            )));
                        }
                        structural_match = Some((
                            Self::WorkInstanceReport(payload),
                            stringify!(WorkInstanceReport),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "received" => {
                    let primary_error =
                        match serde_json::from_value::<WorkInstanceReport>(value.clone()) {
                            Ok(payload) => return Ok(Self::WorkInstanceReport(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<WorkNoReport>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "report",
                                "received",
                                first_name,
                                stringify!(WorkNoReport),
                            )));
                        }
                        structural_match =
                            Some((Self::WorkNoReport(payload), stringify!(WorkNoReport)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "report",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "report",
                "`",
            ))),
        }
    }
}
///No Instance of the Workspace has sent a work report.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkNoReport {
    pub report: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkInstanceReport {
    ///Whether the Workspace still holds that Instance. When false the report belongs to an Instance it released or replaced, never to its current one.
    pub current_instance: bool,
    ///The Instance that sent the report.
    pub instance: String,
    pub report: serde_json::Value,
    pub reported_at: String,
    pub repositories: Vec<WorkRepository>,
}
#[derive(Debug, Clone)]
pub enum WorkRepository {
    WorkRepositoryRead(WorkRepositoryRead),
    WorkRepositoryUnreadable(WorkRepositoryUnreadable),
}
impl serde::Serialize for WorkRepository {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::WorkRepositoryRead(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WorkRepositoryRead),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("git") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "read") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "git",
                            stringify!(WorkRepositoryRead),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "git",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "git".to_string(),
                            serde_json::Value::String("read".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::WorkRepositoryUnreadable(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WorkRepositoryUnreadable),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("git") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "unreadable") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "git",
                            stringify!(WorkRepositoryUnreadable),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "git",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "git".to_string(),
                            serde_json::Value::String("unreadable".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for WorkRepository {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("git") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "git",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "read" => {
                    let primary_error =
                        match serde_json::from_value::<WorkRepositoryRead>(value.clone()) {
                            Ok(payload) => return Ok(Self::WorkRepositoryRead(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) =
                        serde_json::from_value::<WorkRepositoryUnreadable>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "git",
                                "read",
                                first_name,
                                stringify!(WorkRepositoryUnreadable),
                            )));
                        }
                        structural_match = Some((
                            Self::WorkRepositoryUnreadable(payload),
                            stringify!(WorkRepositoryUnreadable),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "unreadable" => {
                    let primary_error =
                        match serde_json::from_value::<WorkRepositoryUnreadable>(value.clone()) {
                            Ok(payload) => {
                                return Ok(Self::WorkRepositoryUnreadable(payload));
                            }
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<WorkRepositoryRead>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "git",
                                "unreadable",
                                first_name,
                                stringify!(WorkRepositoryRead),
                            )));
                        }
                        structural_match = Some((
                            Self::WorkRepositoryRead(payload),
                            stringify!(WorkRepositoryRead),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "git",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "git",
                "`",
            ))),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkRepositoryUnreadable {
    pub because: String,
    pub git: serde_json::Value,
    pub repository: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkRepositoryRead {
    pub branch: Option<String>,
    pub changed: WorkChanges,
    pub committed: WorkCommits,
    pub git: serde_json::Value,
    pub pushed: Option<String>,
    pub repository: String,
    pub staged: WorkChanges,
    ///Constraint: minimum=0
    pub stashed: i64,
    ///Constraint: minimum=0
    pub untracked: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkCommits {
    ///Constraint: minimum=0
    pub added: i64,
    ///Constraint: minimum=0
    pub commits: i64,
    ///Constraint: minimum=0
    pub removed: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkChanges {
    ///Constraint: minimum=0
    pub added: i64,
    ///Constraint: minimum=0
    pub files: i64,
    ///Constraint: minimum=0
    pub removed: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerTestResult {
    pub agent: String,
    pub branch: Option<String>,
    pub brief: String,
    pub correlation: Option<String>,
    pub elapsing: Option<String>,
    pub matches: bool,
    ///What a firing would do now: open a Workspace, continue the waiting Session of the open one it correlates to, start a new Session there, or ignore a correlation miss.
    pub would: TriggerTestResultWould,
}
///What a firing would do now: open a Workspace, continue the waiting Session of the open one it correlates to, start a new Session there, or ignore a correlation miss.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerTestResultWould {
    #[default]
    #[serde(rename = "open_workspace")]
    OpenWorkspace,
    #[serde(rename = "continue_session")]
    ContinueSession,
    #[serde(rename = "new_session")]
    NewSession,
    #[serde(rename = "ignore")]
    Ignore,
}
impl TriggerTestResultWould {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenWorkspace => "open_workspace",
            Self::ContinueSession => "continue_session",
            Self::NewSession => "new_session",
            Self::Ignore => "ignore",
        }
    }
}
impl ::std::fmt::Display for TriggerTestResultWould {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerTestResultWould {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TriggerTest {
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub agent: Option<Option<String>>,
    ///A trigger file, to test the Trigger as it declares it rather than as it was applied.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub declared: Option<Option<TriggerFile>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub event: Option<Option<uuid::Uuid>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub instruction: Option<Option<String>>,
    ///The GitHub Integration the issue is read through; named with `issue`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub integration: Option<Option<String>>,
    ///Render against the Event a dispatch of this issue would record, recording nothing. Excludes `event`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub issue: Option<Option<i64>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerFile {
    ///Each Trigger the file applies, keyed by its name; `{}` removes every Trigger a file applied.
    pub triggers: TriggerFileTriggers,
}
///Each Trigger the file applies, keyed by its name; `{}` removes every Trigger a file applied.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TriggerFileTriggers {
    /// Additional properties matching the spec's
    /// `additionalProperties` value schema.
    #[serde(flatten)]
    pub additional_properties:
        std::collections::BTreeMap<String, TriggerFileTriggersAdditionalProperty>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerFileTriggersAdditionalProperty {
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allows: Option<Vec<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub branch: Option<Option<String>>,
    pub brief: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub correlation: Option<Option<String>>,
    pub filter: serde_json::Value,
    ///The mode its Sessions run in. Omitted, the Agent's, then the Harness's default.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub mode: Option<Option<String>>,
    ///The model its Sessions run with. Omitted, the Agent's, then the Harness's default.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub model: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub on_miss: Option<Option<TriggerFileTriggersAdditionalPropertyOnMiss>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub on_open_workspace: Option<Option<TriggerFileTriggersAdditionalPropertyOnOpenWorkspace>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub profile: Option<Option<String>>,
    pub project: String,
    ///The thought level its Sessions run at. Omitted, the Agent's, then the Harness's default.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub thought_level: Option<Option<String>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerFileTriggersAdditionalPropertyOnOpenWorkspace {
    #[default]
    #[serde(rename = "continue")]
    Continue_,
    #[serde(rename = "new-session")]
    NewSession,
    #[serde(rename = "null")]
    NullValue,
}
impl TriggerFileTriggersAdditionalPropertyOnOpenWorkspace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Continue_ => "continue",
            Self::NewSession => "new-session",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for TriggerFileTriggersAdditionalPropertyOnOpenWorkspace {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerFileTriggersAdditionalPropertyOnOpenWorkspace {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerFileTriggersAdditionalPropertyOnMiss {
    #[default]
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "ignore")]
    Ignore,
    #[serde(rename = "null")]
    NullValue,
}
impl TriggerFileTriggersAdditionalPropertyOnMiss {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Ignore => "ignore",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for TriggerFileTriggersAdditionalPropertyOnMiss {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerFileTriggersAdditionalPropertyOnMiss {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamSubscription {
    ///The last Transcript cursor the tab saw. Absent means from the beginning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub kind: StreamSubscriptionKind,
    ///Comma-separated Transcript kinds, as the Transcript read takes them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kinds: Option<String>,
    pub organization: String,
    ///The name a `transcript` subscription's follower joins presence under, refused as the Transcript read's `as` is. Absent means anonymous.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participant: Option<String>,
    ///The Workspace whose Transcript a `transcript` subscription follows, addressed as the Transcript read addresses it. Required for `transcript`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}
impl StreamSubscription {
    /// Construct this request with every required wire field.
    pub fn new(kind: StreamSubscriptionKind, organization: String) -> Self {
        Self {
            kind,
            organization,
            after: None,
            kinds: None,
            participant: None,
            workspace: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(
        kind: StreamSubscriptionKind,
        organization: String,
    ) -> StreamSubscriptionBuilder {
        StreamSubscriptionBuilder::new(kind, organization)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct StreamSubscriptionBuilder {
    value: StreamSubscription,
}
impl StreamSubscriptionBuilder {
    /// Start a builder with every required wire field.
    pub fn new(kind: StreamSubscriptionKind, organization: String) -> Self {
        Self {
            value: StreamSubscription::new(kind, organization),
        }
    }
    #[doc = concat!("Set the optional `", "after", "` request field.")]
    #[must_use]
    pub fn after(mut self, after: String) -> Self {
        self.value.after = Some(after);
        self
    }
    #[doc = concat!("Set the optional `", "kinds", "` request field.")]
    #[must_use]
    pub fn kinds(mut self, kinds: String) -> Self {
        self.value.kinds = Some(kinds);
        self
    }
    #[doc = concat!("Set the optional `", "participant", "` request field.")]
    #[must_use]
    pub fn participant(mut self, participant: String) -> Self {
        self.value.participant = Some(participant);
        self
    }
    #[doc = concat!("Set the optional `", "workspace", "` request field.")]
    #[must_use]
    pub fn workspace(mut self, workspace: String) -> Self {
        self.value.workspace = Some(workspace);
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> StreamSubscription {
        self.value
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum StreamSubscriptionKind {
    #[default]
    #[serde(rename = "notices")]
    Notices,
    #[serde(rename = "transcript")]
    Transcript,
}
impl StreamSubscriptionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Notices => "notices",
            Self::Transcript => "transcript",
        }
    }
}
impl ::std::fmt::Display for StreamSubscriptionKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for StreamSubscriptionKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Started {
    pub agent: Settled,
    pub organization: Settled,
    pub project: Settled,
    pub session: Session,
    pub workspace: Workspace,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Settled {
    ///Whether the start declared it, rather than finding it declared.
    pub created: bool,
    pub name: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartPlan {
    pub agent: AgentDeclaration,
    ///The Brief the Workspace starts with, handed to the agent exactly as given.
    ///Constraint: minLength=1
    pub brief: String,
    ///Provider Credentials the Organization holds from this start on, each replacing any it holds under the same variable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<Vec<StartPlanCredentialsItem>>,
    ///Constraint: minLength=1
    pub organization: String,
    pub project: ProjectDeclaration,
}
impl StartPlan {
    /// Construct this request with every required wire field.
    pub fn new(
        agent: AgentDeclaration,
        brief: String,
        organization: String,
        project: ProjectDeclaration,
    ) -> Self {
        Self {
            agent,
            brief,
            organization,
            project,
            credentials: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(
        agent: AgentDeclaration,
        brief: String,
        organization: String,
        project: ProjectDeclaration,
    ) -> StartPlanBuilder {
        StartPlanBuilder::new(agent, brief, organization, project)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct StartPlanBuilder {
    value: StartPlan,
}
impl StartPlanBuilder {
    /// Start a builder with every required wire field.
    pub fn new(
        agent: AgentDeclaration,
        brief: String,
        organization: String,
        project: ProjectDeclaration,
    ) -> Self {
        Self {
            value: StartPlan::new(agent, brief, organization, project),
        }
    }
    #[doc = concat!("Set the optional `", "credentials", "` request field.")]
    #[must_use]
    pub fn credentials(mut self, credentials: Vec<StartPlanCredentialsItem>) -> Self {
        self.value.credentials = Some(credentials);
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> StartPlan {
        self.value
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartPlanCredentialsItem {
    ///Constraint: minLength=1
    pub secret: String,
    ///Constraint: minLength=1
    pub variable: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryResolutionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<RepositoryPurpose>,
    pub repositories: Vec<String>,
}
impl RepositoryResolutionRequest {
    /// Construct this request with every required wire field.
    pub fn new(repositories: Vec<String>) -> Self {
        Self {
            repositories,
            purpose: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(repositories: Vec<String>) -> RepositoryResolutionRequestBuilder {
        RepositoryResolutionRequestBuilder::new(repositories)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct RepositoryResolutionRequestBuilder {
    value: RepositoryResolutionRequest,
}
impl RepositoryResolutionRequestBuilder {
    /// Start a builder with every required wire field.
    pub fn new(repositories: Vec<String>) -> Self {
        Self {
            value: RepositoryResolutionRequest::new(repositories),
        }
    }
    #[doc = concat!("Set the optional `", "purpose", "` request field.")]
    #[must_use]
    pub fn purpose(mut self, purpose: RepositoryPurpose) -> Self {
        self.value.purpose = Some(purpose);
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> RepositoryResolutionRequest {
        self.value
    }
}
///`declaration`, the default, takes every transport a Project does. `public_setup` takes only an HTTPS address or GitHub shorthand, since public-repository setup selects no Integration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum RepositoryPurpose {
    #[default]
    #[serde(rename = "declaration")]
    Declaration,
    #[serde(rename = "public_setup")]
    PublicSetup,
}
impl RepositoryPurpose {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Declaration => "declaration",
            Self::PublicSetup => "public_setup",
        }
    }
}
impl ::std::fmt::Display for RepositoryPurpose {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for RepositoryPurpose {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryResolution {
    pub repositories: Vec<ResolvedRepository>,
    pub warnings: Vec<RepositoryWarning>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolvedRepository {
    ///What a declaration holds and a checkout clones.
    pub address: String,
    ///The directory under the Workspace the repository is checked out into.
    pub checkout_directory: String,
    pub given: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryWarning {
    pub code: RepositoryWarningCode,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum RepositoryWarningCode {
    #[default]
    #[serde(rename = "no_app_push_authority")]
    NoAppPushAuthority,
}
impl RepositoryWarningCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoAppPushAuthority => "no_app_push_authority",
        }
    }
}
impl ::std::fmt::Display for RepositoryWarningCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for RepositoryWarningCode {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryDiff {
    pub diff: String,
    pub files: Vec<FileStat>,
    pub repository: String,
    pub truncated: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileStat {
    ///Constraint: minimum=0
    pub added: Option<i64>,
    pub path: String,
    ///Constraint: minimum=0
    pub removed: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Queue {
    pub active_work: ActiveWork,
    pub instances: QueueInstances,
    pub queued: Vec<QueuedSession>,
    pub unbriefed: Vec<UnbriefedSession>,
    pub waiting: Vec<WaitingSession>,
    ///The last recorded dispatch configuration, or null when none is known. This record does not establish work-role liveness.
    pub work_role: Option<QueueWorkRole>,
}
///The last recorded dispatch configuration, or null when none is known. This record does not establish work-role liveness.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueWorkRole {
    ///Constraint: minimum=1
    pub active_work_slots: i64,
    ///The Compute driver it provisions Instances with: the Environment work is provisioned in, read-only.
    pub driver: String,
    pub serialized_harnesses: Vec<String>,
}
///A Session between Turns. Held Messages request an Active-Work Slot and have a global position when eligible.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WaitingSession {
    pub agent: String,
    pub enqueued_at: String,
    ///Constraint: pattern=`^[a-z]+-[a-z]+-[a-z]{8}$`
    pub name: String,
    ///When the oldest input held for its next turn arrived, or null while it is only waiting for a turn.
    pub pending_since: Option<String>,
    ///Global order among eligible requests for an Active-Work Slot, including queued Sessions, Held Messages and first Briefs. Retained while slots are full; null when blocked, not requesting a slot, or dispatch configuration is unknown.
    ///Constraint: minimum=1
    pub position: Option<i64>,
    ///Current blockers and predecessors for this request, computed by the scheduler. Other Organizations are counted without naming their Sessions.
    pub reasons: Vec<QueueReason>,
    pub workspace: uuid::Uuid,
}
///A Session preparing or ready for its first Turn. Provisioning is unnumbered; a written Brief competes for an Active-Work Slot once the harness is ready.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnbriefedSession {
    pub agent: String,
    ///When its unstarted Brief was written, once the harness is ready, independently of later Held Messages.
    pub brief_since: Option<String>,
    pub enqueued_at: String,
    ///Constraint: pattern=`^[a-z]+-[a-z]+-[a-z]{8}$`
    pub name: String,
    ///When the oldest message held for its Brief arrived, or null while it holds none.
    pub pending_since: Option<String>,
    ///Global order among eligible requests for an Active-Work Slot, including queued Sessions, Held Messages and first Briefs. Retained while slots are full; null when blocked, not requesting a slot, or dispatch configuration is unknown.
    ///Constraint: minimum=1
    pub position: Option<i64>,
    ///The step it is preparing on: provisioning from claim until its supervisor connects, cloning until its checkout is reported, starting_harness until the harness is up, and harness_ready once it is.
    pub preparing: Option<UnbriefedSessionPreparing>,
    ///Current blockers and predecessors for this request, computed by the scheduler. Other Organizations are counted without naming their Sessions.
    pub reasons: Vec<QueueReason>,
    pub workspace: uuid::Uuid,
}
///The step it is preparing on: provisioning from claim until its supervisor connects, cloning until its checkout is reported, starting_harness until the harness is up, and harness_ready once it is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum UnbriefedSessionPreparing {
    #[default]
    #[serde(rename = "provisioning")]
    Provisioning,
    #[serde(rename = "cloning")]
    Cloning,
    #[serde(rename = "starting_harness")]
    StartingHarness,
    #[serde(rename = "harness_ready")]
    HarnessReady,
    #[serde(rename = "null")]
    NullValue,
}
impl UnbriefedSessionPreparing {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Provisioning => "provisioning",
            Self::Cloning => "cloning",
            Self::StartingHarness => "starting_harness",
            Self::HarnessReady => "harness_ready",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for UnbriefedSessionPreparing {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for UnbriefedSessionPreparing {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueuedSession {
    pub agent: String,
    pub enqueued_at: String,
    ///Constraint: pattern=`^[a-z]+-[a-z]+-[a-z]{8}$`
    pub name: String,
    ///Global order among eligible requests for an Active-Work Slot, including queued Sessions, Held Messages and first Briefs. Retained while slots are full; null when blocked, not requesting a slot, or dispatch configuration is unknown.
    ///Constraint: minimum=1
    pub position: Option<i64>,
    ///Current blockers and predecessors for this request, computed by the scheduler. Other Organizations are counted without naming their Sessions.
    pub reasons: Vec<QueueReason>,
    pub workspace: uuid::Uuid,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueInstances {
    ///The Instances counted against it, idle ones retained for Unpublished Work included.
    ///Constraint: minimum=0
    pub count: i64,
    ///The counted Instances that can be named.
    pub counted: Vec<String>,
    ///The Organization's live Instance limit, or null when unbounded.
    ///Constraint: minimum=1
    pub limit: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Posted {
    ///The Held Message the post left, if a Turn could not take it at once.
    pub held_message: Option<HeldMessage>,
    ///The Session the message started or woke, if it started or woke one.
    pub session: Option<Session>,
}
///A Workspace and the first Session its open enqueued, in one transaction.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Opened {
    pub session: Session,
    pub workspace: Workspace,
}
pub type ListWorkspacesResponse = Vec<WorkspaceListed>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceListed {
    pub checkout: Checkout,
    pub continued_by: Vec<String>,
    pub continues: Option<uuid::Uuid>,
    pub correlation: Option<String>,
    ///Why its Instance is held for work that exists nowhere else, if it is.
    pub held: Option<String>,
    ///The messages the unfinished Session cannot take yet, in arrival order.
    pub held_messages: Vec<HeldMessage>,
    pub id: uuid::Uuid,
    ///The Instance its Sessions execute on, while one is kept.
    pub instance: Option<String>,
    pub last_active_at: String,
    ///Constraint: pattern=`^[a-z]+-[a-z]+-[a-z]{8}$`
    pub name: String,
    pub opened_at: String,
    ///The Agent its first Session runs unless that Session names another.
    pub opened_with: String,
    pub organization: String,
    pub profile: Option<String>,
    pub project: String,
    ///One per repository fixed on the Workspace, in the checkout's order.
    pub pull_requests: Vec<PullRequestAvailability>,
    ///Where that Session stands in the Organization's queue; null when it is in none of the queue's lists.
    pub queue: Option<QueueStanding>,
    pub sealed_at: Option<String>,
    ///The Workspace's latest Session, as the Session read answers it; null before it has one.
    pub session: Option<Session>,
    ///The Event a firing opened it for, or the Brief's author an operator opened it with. Null when neither names anyone.
    pub started_by: Option<String>,
    pub state: WorkspaceState,
    ///The one Session the Workspace has not let go of, while it has one.
    pub unfinished_session: Option<UnfinishedSession>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Workspace {
    pub checkout: Checkout,
    pub continued_by: Vec<String>,
    pub continues: Option<uuid::Uuid>,
    pub correlation: Option<String>,
    ///Why its Instance is held for work that exists nowhere else, if it is.
    pub held: Option<String>,
    ///The messages the unfinished Session cannot take yet, in arrival order.
    pub held_messages: Vec<HeldMessage>,
    pub id: uuid::Uuid,
    ///The Instance its Sessions execute on, while one is kept.
    pub instance: Option<String>,
    pub last_active_at: String,
    ///Constraint: pattern=`^[a-z]+-[a-z]+-[a-z]{8}$`
    pub name: String,
    pub opened_at: String,
    ///The Agent its first Session runs unless that Session names another.
    pub opened_with: String,
    pub organization: String,
    pub profile: Option<String>,
    pub project: String,
    ///One per repository fixed on the Workspace, in the checkout's order.
    pub pull_requests: Vec<PullRequestAvailability>,
    pub sealed_at: Option<String>,
    ///The Event a firing opened it for, or the Brief's author an operator opened it with. Null when neither names anyone.
    pub started_by: Option<String>,
    pub state: WorkspaceState,
    ///The one Session the Workspace has not let go of, while it has one.
    pub unfinished_session: Option<UnfinishedSession>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum WorkspaceState {
    #[default]
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "sealed")]
    Sealed,
}
impl WorkspaceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Sealed => "sealed",
        }
    }
}
impl ::std::fmt::Display for WorkspaceState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for WorkspaceState {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnfinishedSession {
    pub id: uuid::Uuid,
    ///Constraint: pattern=`^[a-z]+-[a-z]+-[a-z]{8}$`
    pub name: String,
    pub preparing: Option<UnfinishedSessionPreparing>,
    ///A trailing Session has answered its Turn while work its agent started still runs: it holds its Active-Work Slot and its Instance, refuses seal, and is waiting once that work settles and its agent falls quiet.
    pub state: UnfinishedSessionState,
}
///A trailing Session has answered its Turn while work its agent started still runs: it holds its Active-Work Slot and its Instance, refuses seal, and is waiting once that work settles and its agent falls quiet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum UnfinishedSessionState {
    #[default]
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "trailing")]
    Trailing,
    #[serde(rename = "waiting")]
    Waiting,
    #[serde(rename = "unbriefed")]
    Unbriefed,
    #[serde(rename = "ended")]
    Ended,
    #[serde(rename = "unreachable")]
    Unreachable,
}
impl UnfinishedSessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Working => "working",
            Self::Trailing => "trailing",
            Self::Waiting => "waiting",
            Self::Unbriefed => "unbriefed",
            Self::Ended => "ended",
            Self::Unreachable => "unreachable",
        }
    }
}
impl ::std::fmt::Display for UnfinishedSessionState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for UnfinishedSessionState {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum UnfinishedSessionPreparing {
    #[default]
    #[serde(rename = "provisioning")]
    Provisioning,
    #[serde(rename = "cloning")]
    Cloning,
    #[serde(rename = "starting_harness")]
    StartingHarness,
    #[serde(rename = "harness_ready")]
    HarnessReady,
    #[serde(rename = "null")]
    NullValue,
}
impl UnfinishedSessionPreparing {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Provisioning => "provisioning",
            Self::Cloning => "cloning",
            Self::StartingHarness => "starting_harness",
            Self::HarnessReady => "harness_ready",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for UnfinishedSessionPreparing {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for UnfinishedSessionPreparing {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PullRequestAvailability {
    ///Unavailable when no inbound GitHub Integration in the Organization watches the repository and none has delivered a pull request from it: kestrel cannot learn its pull requests, which says nothing about whether any exist.
    pub availability: PullRequestAvailabilityAvailability,
    ///The pull requests learned from the declared branch in this repository; null when unavailable.
    pub known: Option<Vec<PullRequest>>,
    pub repository: String,
}
///Unavailable when no inbound GitHub Integration in the Organization watches the repository and none has delivered a pull request from it: kestrel cannot learn its pull requests, which says nothing about whether any exist.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum PullRequestAvailabilityAvailability {
    #[default]
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "unavailable")]
    Unavailable,
}
impl PullRequestAvailabilityAvailability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}
impl ::std::fmt::Display for PullRequestAvailabilityAvailability {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for PullRequestAvailabilityAvailability {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///A Workspace's current value for one pull request, learned from a verified GitHub Event.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PullRequest {
    ///The Event the value was learned from.
    pub event: uuid::Uuid,
    pub head_branch: String,
    pub head_revision: String,
    ///Constraint: minimum=1
    pub number: i64,
    ///The head repository fixed on the Workspace.
    pub repository: String,
    pub state: PullRequestState,
    pub title: String,
    ///When GitHub last changed it, as the Event or a read back of the pull request said.
    pub updated_at: String,
    pub url: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeldMessage {
    pub edited_at: Option<String>,
    ///The Workspace's own sequence. Stable and never reused: a taken or withdrawn message keeps it.
    pub id: i64,
    pub message: String,
    pub participant: String,
    pub posted_at: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Checkout {
    pub base: String,
    pub branch: String,
    pub repositories: Vec<String>,
}
///A Workspace's Session as the queue sees it: queued with a position or reasons, waiting with reasons and held input, or unbriefed with held input.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueStanding {
    ///When the oldest input held for its next turn arrived.
    pub pending_since: Option<String>,
    ///Its global place among eligible requests for an Active-Work Slot; null when blocked, not requesting a slot, or dispatch configuration is unknown.
    ///Constraint: minimum=1
    pub position: Option<i64>,
    pub reasons: Vec<QueueReason>,
}
///One reason a Session has not started, for a Client to phrase.
#[derive(Debug, Clone)]
pub enum QueueReason {
    QueueReasonDependencies(QueueReasonDependencies),
    QueueReasonSubscriptionProfile(QueueReasonSubscriptionProfile),
    QueueReasonInstanceArchiving(QueueReasonInstanceArchiving),
    QueueReasonLiveInstanceLimit(QueueReasonLiveInstanceLimit),
    QueueReasonActiveWorkSlots(QueueReasonActiveWorkSlots),
    QueueReasonAhead(QueueReasonAhead),
}
impl Serialize for QueueReason {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::QueueReasonDependencies(value) => serde::Serialize::serialize(value, serializer),
            Self::QueueReasonSubscriptionProfile(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::QueueReasonInstanceArchiving(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::QueueReasonLiveInstanceLimit(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::QueueReasonActiveWorkSlots(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::QueueReasonAhead(value) => serde::Serialize::serialize(value, serializer),
        }
    }
}
impl<'de> Deserialize<'de> for QueueReason {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn exact_json_integer(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
        }
        fn json_numbers_have_same_value(
            encoded: &serde_json::Number,
            input: &serde_json::Number,
        ) -> bool {
            match (exact_json_integer(encoded), exact_json_integer(input)) {
                (Some(encoded), Some(input)) => encoded == input,
                (Some(encoded), None) => input.as_f64().is_some_and(|input| {
                    input.is_finite() && input.fract() == 0.0 && input as i128 == encoded
                }),
                (None, Some(input)) => encoded.as_f64().is_some_and(|encoded| {
                    encoded.is_finite() && encoded.fract() == 0.0 && encoded as i128 == input
                }),
                (None, None) => encoded.as_f64() == input.as_f64(),
            }
        }
        /// `nulls_may_be_absent` also accepts an input `null` that the
        /// branch omits, as a skipped `None` does. Extra encoded
        /// keys are allowed only by the pre-existing anyOf match.
        fn preserves_complete_json_input(
            encoded: &serde_json::Value,
            input: &serde_json::Value,
            nulls_may_be_absent: bool,
            encoded_keys_may_be_extra: bool,
        ) -> bool {
            match (encoded, input) {
                (serde_json::Value::Object(encoded), serde_json::Value::Object(input)) => {
                    (encoded_keys_may_be_extra
                        || encoded.iter().all(|(key, value)| {
                            input.contains_key(key) || (nulls_may_be_absent && value.is_null())
                        }))
                        && input.iter().all(|(key, value)| match encoded.get(key) {
                            Some(encoded_value) => preserves_complete_json_input(
                                encoded_value,
                                value,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            ),
                            None => nulls_may_be_absent && value.is_null(),
                        })
                }
                (serde_json::Value::Array(encoded), serde_json::Value::Array(input)) => {
                    encoded.len() == input.len()
                        && encoded.iter().zip(input).all(|(encoded, input)| {
                            preserves_complete_json_input(
                                encoded,
                                input,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            )
                        })
                }
                (serde_json::Value::Number(encoded), serde_json::Value::Number(input)) => {
                    json_numbers_have_same_value(encoded, input)
                }
                _ => encoded == input,
            }
        }
        let input = <serde_json::Value as Deserialize>::deserialize(deserializer)?;
        let mut matched = None;
        let mut equivalent = None;
        let mut equivalent_matches = 0usize;
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"dependencies\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<QueueReasonDependencies>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(QueueReason),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::QueueReasonDependencies(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::QueueReasonDependencies(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"subscription_profile\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<QueueReasonSubscriptionProfile>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(QueueReason),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::QueueReasonSubscriptionProfile(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::QueueReasonSubscriptionProfile(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"instance_archiving\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<QueueReasonInstanceArchiving>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(QueueReason),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::QueueReasonInstanceArchiving(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::QueueReasonInstanceArchiving(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"live_instance_limit\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<QueueReasonLiveInstanceLimit>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(QueueReason),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::QueueReasonLiveInstanceLimit(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::QueueReasonLiveInstanceLimit(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"active_work_slots\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<QueueReasonActiveWorkSlots>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(QueueReason),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::QueueReasonActiveWorkSlots(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::QueueReasonActiveWorkSlots(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"ahead\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<QueueReasonAhead>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(QueueReason),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::QueueReasonAhead(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::QueueReasonAhead(candidate));
                    }
                    _ => {}
                }
            }
        }
        if let Some(matched) = matched {
            return Ok(matched);
        }
        if equivalent_matches > 1 {
            return Err(serde::de::Error::custom(concat!(
                "ambiguous oneOf value for ",
                stringify!(QueueReason),
                ": more than one branch preserved an equivalent input",
            )));
        }
        equivalent.ok_or_else(|| {
            serde::de::Error::custom(concat!(
                "no oneOf branch for ",
                stringify!(QueueReason),
                " preserved the complete input",
            ))
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueReasonSubscriptionProfile {
    pub kind: serde_json::Value,
    pub profile: String,
    ///The Working or Trailing Session holding the Profile when it belongs to this Organization; otherwise null.
    pub session: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueReasonLiveInstanceLimit {
    pub kind: serde_json::Value,
    ///The Organization's live Instance limit, reached with no idle Instance known recoverable.
    ///Constraint: minimum=1
    pub limit: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueReasonInstanceArchiving {
    ///The idle Instance being archived, or that dispatch will archive, to make room.
    pub instance: String,
    pub kind: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueReasonDependencies {
    pub kind: serde_json::Value,
    ///The blocking Sessions that have not ended successfully.
    pub sessions: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueReasonAhead {
    ///Earlier eligible slot requests in other Organizations, omitted when zero.
    ///Constraint: minimum=1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elsewhere: Option<i64>,
    pub kind: serde_json::Value,
    ///Earlier eligible slot requests in this Organization, in global dispatch order.
    pub sessions: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueueReasonActiveWorkSlots {
    pub kind: serde_json::Value,
    ///The Active-Work Slot limit, every slot occupied.
    ///Constraint: minimum=1
    pub limit: i64,
}
pub type ListTriggersResponse = Vec<Trigger>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Trigger {
    ///Whether the filter fires for Events from people outside the Organization.
    pub admits_outsiders: bool,
    pub agent: String,
    pub allows: Vec<String>,
    ///Whether a trigger file declared it, rather than flags.
    pub applied: bool,
    pub branch: Option<String>,
    pub brief: String,
    pub correlation: Option<String>,
    pub cron: Option<String>,
    pub declared_at: String,
    pub disabled_because: Option<String>,
    pub every: Option<String>,
    pub filter: Option<serde_json::Value>,
    pub firing_budget: TriggerFiringBudget,
    pub id: uuid::Uuid,
    ///The mode its Sessions run in, or null for the Agent's or Harness's default.
    pub mode: Option<String>,
    ///The model its Sessions run with, or null for the Agent's or Harness's default.
    pub model: Option<String>,
    pub name: String,
    pub on_miss: Option<TriggerOnMiss>,
    pub on_open_workspace: TriggerOnOpenWorkspace,
    pub organization: String,
    pub profile: Option<String>,
    pub project: String,
    pub state: TriggerState,
    ///The thought level its Sessions run at, or null for the Agent's or Harness's default.
    pub thought_level: Option<String>,
    pub zone: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerState {
    #[default]
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled:operator")]
    DisabledOperator,
    #[serde(rename = "disabled:firing-budget")]
    DisabledFiringBudget,
}
impl TriggerState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::DisabledOperator => "disabled:operator",
            Self::DisabledFiringBudget => "disabled:firing-budget",
        }
    }
}
impl ::std::fmt::Display for TriggerState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerState {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerOnOpenWorkspace {
    #[default]
    #[serde(rename = "continue")]
    Continue_,
    #[serde(rename = "new-session")]
    NewSession,
}
impl TriggerOnOpenWorkspace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Continue_ => "continue",
            Self::NewSession => "new-session",
        }
    }
}
impl ::std::fmt::Display for TriggerOnOpenWorkspace {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerOnOpenWorkspace {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerOnMiss {
    #[default]
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "ignore")]
    Ignore,
    #[serde(rename = "null")]
    NullValue,
}
impl TriggerOnMiss {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Ignore => "ignore",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for TriggerOnMiss {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerOnMiss {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerFiringBudget {
    ///Constraint: minimum=1
    pub limit: i64,
    pub window: String,
}
pub type ListSubscriptionProfilesResponse = Vec<SubscriptionProfileListed>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscriptionProfileListed {
    pub holds: Vec<SubscriptionProfileLogin>,
    pub id: uuid::Uuid,
    pub name: String,
    pub owner: String,
    pub owner_operator: Option<uuid::Uuid>,
}
///A login as everything but the Session that carries it sees it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscriptionProfileLogin {
    pub kind: SubscriptionProfileLoginKind,
    ///The variable, or the path beneath the agent's home.
    pub name: String,
    ///When it was last held, whether by a person or by a Session handing back a refreshed login.
    pub set_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SubscriptionProfileLoginKind {
    #[default]
    #[serde(rename = "variable")]
    Variable,
    #[serde(rename = "file")]
    File,
}
impl SubscriptionProfileLoginKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Variable => "variable",
            Self::File => "file",
        }
    }
}
impl ::std::fmt::Display for SubscriptionProfileLoginKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SubscriptionProfileLoginKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscriptionProfile {
    pub id: uuid::Uuid,
    pub name: String,
    pub owner: String,
    pub owner_operator: Option<uuid::Uuid>,
}
pub type ListSessionsResponse = Vec<Session>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub agent: String,
    ///Option changes a person asked for while the Session is live, held until the harness answers each (ADR-0041).
    pub changing_options: Vec<ChangingOption>,
    ///The commands the harness offers for this Session.
    pub commands: Vec<SessionCommand>,
    pub connected_at: Option<String>,
    ///The Sessions this one was declared to wait on, whether or not they have ended.
    pub depends_on: Vec<SessionReference>,
    ///Why the Session failed, from the evidence its supervisor established. Null when it has not failed, or failed with no evidence; its presence never makes the read fail (ADR-0052).
    pub diagnostic: Option<Diagnostic>,
    pub ended_at: Option<String>,
    pub enqueued_at: String,
    pub exit: Option<Exit>,
    pub harness: String,
    pub id: uuid::Uuid,
    pub instance: Option<String>,
    ///Who asked the working Turn to stop, while the request is in flight.
    pub interrupting: Option<Interrupting>,
    ///When the trailing Session's agent last showed activity; null in every other state.
    pub last_activity_at: Option<String>,
    pub lease_expires_at: Option<String>,
    pub message_buffering: bool,
    ///What the Session names for its Harness's mode, or null for its Harness's default.
    pub mode: Option<String>,
    ///What the Session names for its Harness's model, or null for its Harness's default.
    pub model: Option<String>,
    ///Constraint: pattern=`^[a-z]+-[a-z]+-[a-z]{8}$`
    pub name: String,
    pub observation: SessionObservation,
    ///The harness's whole config-option list (ADR-0041).
    pub options: Vec<SessionOption>,
    pub outcome_message: Option<String>,
    ///The step an unbriefed Session is preparing on: provisioning from claim until its supervisor connects, cloning until its checkout is reported, starting_harness until the harness is up, and harness_ready once it is. Null in every other state.
    pub preparing: Option<SessionPreparing>,
    pub started_at: Option<String>,
    ///A trailing Session has answered its Turn while work its agent started still runs: it holds its Active-Work Slot and its Instance, refuses seal, and is waiting once that work settles and its agent falls quiet.
    pub state: SessionState,
    pub supervisor: Option<String>,
    pub supervisor_version: Option<String>,
    pub thought_buffering: bool,
    ///What the Session names for its Harness's thought level, or null for its Harness's default.
    pub thought_level: Option<String>,
    ///The harness's own name for the conversation, or null until it says one.
    pub title: Option<String>,
    ///Tool calls open in the current observation; empty whenever observation is unavailable.
    pub tools: Vec<RunningTool>,
    ///Adapter units open in the current observation, such as a Claude background task or subagent; they keep a trailing Session trailing. Empty whenever observation is unavailable.
    pub units: Vec<RunningUnit>,
    ///What the Harness has spent on the Session: the live figure while its agent works, otherwise the latest its Turn answer or the end of its trailing work reported.
    pub usage: Option<Usage>,
    ///The Model-category option's current value, kept current by the supervisor (ADR-0041).
    pub worked_model: Option<String>,
    pub workspace: uuid::Uuid,
}
///A trailing Session has answered its Turn while work its agent started still runs: it holds its Active-Work Slot and its Instance, refuses seal, and is waiting once that work settles and its agent falls quiet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SessionState {
    #[default]
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "trailing")]
    Trailing,
    #[serde(rename = "waiting")]
    Waiting,
    #[serde(rename = "unbriefed")]
    Unbriefed,
    #[serde(rename = "ended")]
    Ended,
    #[serde(rename = "unreachable")]
    Unreachable,
}
impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Working => "working",
            Self::Trailing => "trailing",
            Self::Waiting => "waiting",
            Self::Unbriefed => "unbriefed",
            Self::Ended => "ended",
            Self::Unreachable => "unreachable",
        }
    }
}
impl ::std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SessionState {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionReference {
    pub id: String,
    pub name: String,
}
///The step an unbriefed Session is preparing on: provisioning from claim until its supervisor connects, cloning until its checkout is reported, starting_harness until the harness is up, and harness_ready once it is. Null in every other state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SessionPreparing {
    #[default]
    #[serde(rename = "provisioning")]
    Provisioning,
    #[serde(rename = "cloning")]
    Cloning,
    #[serde(rename = "starting_harness")]
    StartingHarness,
    #[serde(rename = "harness_ready")]
    HarnessReady,
    #[serde(rename = "null")]
    NullValue,
}
impl SessionPreparing {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Provisioning => "provisioning",
            Self::Cloning => "cloning",
            Self::StartingHarness => "starting_harness",
            Self::HarnessReady => "harness_ready",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for SessionPreparing {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SessionPreparing {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///A harness config option, whole: what it is, what it may be and what it is now.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionOption {
    ///`mode`, `model`, `model_config`, `thought_level`, or the harness's own string, which may begin with `_`.
    pub category: Option<String>,
    pub current: SessionOptionCurrent,
    pub description: Option<String>,
    ///The offered values of a grouped select; empty for an ungrouped one.
    pub groups: Vec<SessionOptionGroup>,
    pub id: String,
    pub kind: SessionOptionKind,
    pub name: String,
    ///The offered values of an ungrouped select; empty for a grouped one and for a boolean.
    pub values: Vec<SessionOptionValue>,
    ///Whether changing this option makes the next Turn re-read the context without the prompt cache. True for a Model, ThoughtLevel or ModelConfig option, except a category kestrel's per-harness table keeps the cache for; at 0.3 that is Claude's ThoughtLevel (ADR-0041).
    pub warns_cache: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SessionOptionKind {
    #[default]
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "boolean")]
    Boolean,
}
impl SessionOptionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Boolean => "boolean",
        }
    }
}
impl ::std::fmt::Display for SessionOptionKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SessionOptionKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionOptionGroup {
    pub group: String,
    pub name: String,
    pub values: Vec<SessionOptionValue>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionOptionValue {
    pub description: Option<String>,
    pub name: String,
    pub value: String,
}
///The current value: a value id for a select, a boolean for a toggle.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SessionOptionCurrent {
    SelectedOptionValue(SelectedOptionValue),
    ToggleOptionValue(ToggleOptionValue),
}
pub type ToggleOptionValue = bool;
pub type SelectedOptionValue = String;
///A command the harness offers for the Session, as a composer lists it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionCommand {
    pub description: String,
    pub input_hint: Option<String>,
    pub name: String,
}
///A person's request that a Session's working Turn stop.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Interrupting {
    pub participant: String,
    pub requested_at: String,
}
///An option change a person asked for while a Session is live, held until the harness answers it (ADR-0041).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChangingOption {
    pub category: String,
    pub option: String,
    pub participant: String,
    pub value: String,
}
pub type ListProviderCredentialsResponse = Vec<ProviderCredential>;
///A credential as everything but the Session that carries it sees it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderCredential {
    pub set_at: String,
    pub variable: String,
}
pub type ListProjectsResponse = Vec<Project>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Project {
    pub branch: String,
    pub id: uuid::Uuid,
    pub name: String,
    pub repositories: Vec<String>,
}
pub type ListOrganizationsResponse = Vec<Organization>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Organization {
    pub id: uuid::Uuid,
    ///Constraint: minimum=1
    pub max_live_instances: Option<i64>,
    pub name: String,
}
pub type ListIntegrationsResponse = Vec<Integration>;
pub type ListHeldInstancesResponse = Vec<HeldInstance>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeldInstance {
    pub because: String,
    pub instance: String,
    pub workspace: uuid::Uuid,
}
pub type ListHarnessesResponse = Vec<HarnessCatalogueEntry>;
pub type ListEventsResponse = Vec<EventRecord>;
pub type ListAgentsResponse = Vec<Agent>;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Agent {
    pub harness: String,
    pub id: uuid::Uuid,
    pub mode: Option<String>,
    pub model: Option<String>,
    pub name: String,
    pub thought_level: Option<String>,
}
///A registration, by the kind of external system it connects to.
#[derive(Debug, Clone)]
pub enum IntegrationRegistration {
    GithubRegistration(GithubRegistration),
    WebhookRegistration(WebhookRegistration),
}
impl serde::Serialize for IntegrationRegistration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::GithubRegistration(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(GithubRegistration),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("kind") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "github") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "kind",
                            stringify!(GithubRegistration),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "kind",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "kind".to_string(),
                            serde_json::Value::String("github".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::WebhookRegistration(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(WebhookRegistration),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("kind") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "webhook") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "kind",
                            stringify!(WebhookRegistration),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "kind",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "kind".to_string(),
                            serde_json::Value::String("webhook".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for IntegrationRegistration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("kind") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "kind",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "github" => {
                    let primary_error =
                        match serde_json::from_value::<GithubRegistration>(value.clone()) {
                            Ok(payload) => return Ok(Self::GithubRegistration(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) =
                        serde_json::from_value::<WebhookRegistration>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "kind",
                                "github",
                                first_name,
                                stringify!(WebhookRegistration),
                            )));
                        }
                        structural_match = Some((
                            Self::WebhookRegistration(payload),
                            stringify!(WebhookRegistration),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "webhook" => {
                    let primary_error =
                        match serde_json::from_value::<WebhookRegistration>(value.clone()) {
                            Ok(payload) => return Ok(Self::WebhookRegistration(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<GithubRegistration>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "kind",
                                "webhook",
                                first_name,
                                stringify!(GithubRegistration),
                            )));
                        }
                        structural_match = Some((
                            Self::GithubRegistration(payload),
                            stringify!(GithubRegistration),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "kind",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "kind",
                "`",
            ))),
        }
    }
}
///A generic endpoint any producer can POST CloudEvents to. It carries inbound only.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookRegistration {
    ///The directions it carries. Absent means inbound.
    ///Constraint: minItems=1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carries: Option<Vec<Direction>>,
    pub kind: serde_json::Value,
    ///Constraint: minLength=1
    pub name: String,
    ///The secret a sender presents as `Authorization: Bearer <secret>`.
    ///Constraint: minLength=1
    pub secret: String,
}
///What maintenance may change; an omitted field keeps its value.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct IntegrationChange {
    ///Every direction it carries afterwards; a generic webhook carries inbound only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carries: Option<Vec<Direction>>,
    ///How often a GitHub Integration's poll reads its App's Delivery log, as a positive duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    ///The revision this change was decided against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
}
///A registered Integration. What it presents to the external system is never part of it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Integration {
    ///The App's own bot account, `<slug>[bot]`, learned from GitHub when a GitHub Integration was registered.
    pub bot_login: Option<String>,
    pub carries: Vec<Direction>,
    ///Why its use is paused now, with the steps that resume it; null while nothing is.
    pub diagnostic: Option<Diagnostic>,
    pub disabled_at: Option<String>,
    pub id: uuid::Uuid,
    pub kind: IntegrationKind,
    pub last_event_refusal: Option<EventRefusal>,
    pub name: String,
    ///How often it is polled, when it is.
    pub polled_every: Option<String>,
    ///The repository a GitHub Integration watches.
    pub repository: Option<String>,
    ///Bumped by every change, so a Client can say which one it decided against.
    pub revision: i64,
    pub state: IntegrationState,
    ///Where on the link GitHub, or a generic producer, can deliver to it. An inbound GitHub Integration is also polled.
    pub webhook_path: Option<String>,
}
///Disabled pauses every use of an Integration and keeps everything it resumes with.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum IntegrationState {
    #[default]
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}
impl IntegrationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}
impl ::std::fmt::Display for IntegrationState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for IntegrationState {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum IntegrationKind {
    #[default]
    #[serde(rename = "github")]
    Github,
    #[serde(rename = "webhook")]
    Webhook,
}
impl IntegrationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Webhook => "webhook",
        }
    }
}
impl ::std::fmt::Display for IntegrationKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for IntegrationKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///The latest Event an Integration refused rather than stored, or the Deliveries it lost to GitHub's retention, visible until acknowledged.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventRefusal {
    ///Constraint: minimum=0
    pub bytes: Option<i64>,
    ///The refused Event's id; null when Deliveries were lost rather than one refused.
    pub id: Option<String>,
    pub observed_at: String,
    pub reason: String,
    pub source: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HarnessCatalogueEntry {
    ///Whether the configured image declares this harness, read from the image at this request. Null only in the catalogue's own data, never in a response.
    pub availability: Option<HarnessAvailability>,
    ///The default ACP command; explicit harness-command configuration remains authoritative.
    pub command: String,
    pub name: String,
    pub sign_in_methods: Vec<SignInMethod>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignInMethod {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_url: Option<url::Url>,
    pub fills: SignInFill,
    pub id: String,
    pub input: SignInMethodInput,
    pub kind: SignInMethodKind,
    pub name: String,
    pub ownership: SignInMethodOwnership,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay: Option<SignInMethodRelay>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SignInMethodRelay {
    #[default]
    #[serde(rename = "claude-setup-token")]
    ClaudeSetupToken,
    #[serde(rename = "codex-device-auth")]
    CodexDeviceAuth,
}
impl SignInMethodRelay {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeSetupToken => "claude-setup-token",
            Self::CodexDeviceAuth => "codex-device-auth",
        }
    }
}
impl ::std::fmt::Display for SignInMethodRelay {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SignInMethodRelay {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SignInMethodOwnership {
    #[default]
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "organization")]
    Organization,
}
impl SignInMethodOwnership {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Organization => "organization",
        }
    }
}
impl ::std::fmt::Display for SignInMethodOwnership {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SignInMethodOwnership {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SignInMethodKind {
    #[default]
    #[serde(rename = "subscription")]
    Subscription,
    #[serde(rename = "key")]
    Key,
}
impl SignInMethodKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Subscription => "subscription",
            Self::Key => "key",
        }
    }
}
impl ::std::fmt::Display for SignInMethodKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SignInMethodKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum SignInMethodInput {
    #[default]
    #[serde(rename = "token")]
    Token,
    #[serde(rename = "file")]
    File,
}
impl SignInMethodInput {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::File => "file",
        }
    }
}
impl ::std::fmt::Display for SignInMethodInput {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for SignInMethodInput {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone)]
pub enum SignInFill {
    SignInVariableFill(SignInVariableFill),
    SignInFileFill(SignInFileFill),
}
impl serde::Serialize for SignInFill {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::SignInVariableFill(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(SignInVariableFill),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("kind") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "variable") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "kind",
                            stringify!(SignInVariableFill),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "kind",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "kind".to_string(),
                            serde_json::Value::String("variable".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::SignInFileFill(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(SignInFileFill),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("kind") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "file") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "kind",
                            stringify!(SignInFileFill),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "kind",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "kind".to_string(),
                            serde_json::Value::String("file".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for SignInFill {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("kind") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "kind",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "variable" => {
                    let primary_error =
                        match serde_json::from_value::<SignInVariableFill>(value.clone()) {
                            Ok(payload) => return Ok(Self::SignInVariableFill(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<SignInFileFill>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "kind",
                                "variable",
                                first_name,
                                stringify!(SignInFileFill),
                            )));
                        }
                        structural_match =
                            Some((Self::SignInFileFill(payload), stringify!(SignInFileFill)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "file" => {
                    let primary_error =
                        match serde_json::from_value::<SignInFileFill>(value.clone()) {
                            Ok(payload) => return Ok(Self::SignInFileFill(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<SignInVariableFill>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "kind",
                                "file",
                                first_name,
                                stringify!(SignInVariableFill),
                            )));
                        }
                        structural_match = Some((
                            Self::SignInVariableFill(payload),
                            stringify!(SignInVariableFill),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "kind",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "kind",
                "`",
            ))),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignInVariableFill {
    pub kind: serde_json::Value,
    pub variable: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignInFileFill {
    pub kind: serde_json::Value,
    pub path: String,
}
///`available` and `not_carried` come from inspecting the configured image's `dev.kestrel.harnesses` label (ADR-0048). `unavailable` means the inspection failed, so nothing is claimed about the harness, and carries why. `unchecked` means the compute driver runs no image.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HarnessAvailability {
    pub diagnostic: Option<Diagnostic>,
    ///The immutable local image ID the reference resolved to when it was inspected.
    pub identity: Option<String>,
    ///The configured image reference.
    pub image: Option<String>,
    pub state: HarnessAvailabilityState,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum HarnessAvailabilityState {
    #[default]
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "not_carried")]
    NotCarried,
    #[serde(rename = "unchecked")]
    Unchecked,
    #[serde(rename = "unavailable")]
    Unavailable,
}
impl HarnessAvailabilityState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NotCarried => "not_carried",
            Self::Unchecked => "unchecked",
            Self::Unavailable => "unavailable",
        }
    }
}
impl ::std::fmt::Display for HarnessAvailabilityState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for HarnessAvailabilityState {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///A connection to GitHub, watching one repository.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubRegistration {
    ///The GitHub API it reaches.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub api: Option<Option<String>>,
    ///The GitHub App's ID.
    pub app_id: i64,
    ///The directions it carries. Absent means both.
    ///Constraint: minItems=1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carries: Option<Vec<Direction>>,
    ///The ID of the installation the App was installed as.
    pub installation: i64,
    ///How often the poll reads the App's Delivery log, as a duration such as `1m` or `PT30S`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub interval: Option<Option<String>>,
    pub kind: serde_json::Value,
    ///Constraint: minLength=1
    pub name: String,
    ///The App's private key, PEM-encoded.
    pub private_key: String,
    ///The repository it watches, as owner/name.
    pub repository: String,
    ///The App's webhook secret. Given one, a Delivery that reaches kestrel's webhook is recorded on arrival; the repository is polled either way.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub webhook_secret: Option<Option<String>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum Direction {
    #[default]
    #[serde(rename = "inbound")]
    Inbound,
    #[serde(rename = "outbound")]
    Outbound,
}
impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}
impl ::std::fmt::Display for Direction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for Direction {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone)]
pub enum Fired {
    FiredOpened(FiredOpened),
    FiredFed(FiredFed),
    FiredIgnored(FiredIgnored),
}
impl Serialize for Fired {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::FiredOpened(value) => serde::Serialize::serialize(value, serializer),
            Self::FiredFed(value) => serde::Serialize::serialize(value, serializer),
            Self::FiredIgnored(value) => serde::Serialize::serialize(value, serializer),
        }
    }
}
impl<'de> Deserialize<'de> for Fired {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn exact_json_integer(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
        }
        fn json_numbers_have_same_value(
            encoded: &serde_json::Number,
            input: &serde_json::Number,
        ) -> bool {
            match (exact_json_integer(encoded), exact_json_integer(input)) {
                (Some(encoded), Some(input)) => encoded == input,
                (Some(encoded), None) => input.as_f64().is_some_and(|input| {
                    input.is_finite() && input.fract() == 0.0 && input as i128 == encoded
                }),
                (None, Some(input)) => encoded.as_f64().is_some_and(|encoded| {
                    encoded.is_finite() && encoded.fract() == 0.0 && encoded as i128 == input
                }),
                (None, None) => encoded.as_f64() == input.as_f64(),
            }
        }
        /// `nulls_may_be_absent` also accepts an input `null` that the
        /// branch omits, as a skipped `None` does. Extra encoded
        /// keys are allowed only by the pre-existing anyOf match.
        fn preserves_complete_json_input(
            encoded: &serde_json::Value,
            input: &serde_json::Value,
            nulls_may_be_absent: bool,
            encoded_keys_may_be_extra: bool,
        ) -> bool {
            match (encoded, input) {
                (serde_json::Value::Object(encoded), serde_json::Value::Object(input)) => {
                    (encoded_keys_may_be_extra
                        || encoded.iter().all(|(key, value)| {
                            input.contains_key(key) || (nulls_may_be_absent && value.is_null())
                        }))
                        && input.iter().all(|(key, value)| match encoded.get(key) {
                            Some(encoded_value) => preserves_complete_json_input(
                                encoded_value,
                                value,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            ),
                            None => nulls_may_be_absent && value.is_null(),
                        })
                }
                (serde_json::Value::Array(encoded), serde_json::Value::Array(input)) => {
                    encoded.len() == input.len()
                        && encoded.iter().zip(input).all(|(encoded, input)| {
                            preserves_complete_json_input(
                                encoded,
                                input,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            )
                        })
                }
                (serde_json::Value::Number(encoded), serde_json::Value::Number(input)) => {
                    json_numbers_have_same_value(encoded, input)
                }
                _ => encoded == input,
            }
        }
        let input = <serde_json::Value as Deserialize>::deserialize(deserializer)?;
        let mut matched = None;
        let mut equivalent = None;
        let mut equivalent_matches = 0usize;
        if input.as_object().is_some_and(|object| {
            true && object.get("outcome").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"opened\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<FiredOpened>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Fired),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::FiredOpened(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::FiredOpened(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("outcome").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"fed\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<FiredFed>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Fired),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::FiredFed(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::FiredFed(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("outcome").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"ignored\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<FiredIgnored>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Fired),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::FiredIgnored(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::FiredIgnored(candidate));
                    }
                    _ => {}
                }
            }
        }
        if let Some(matched) = matched {
            return Ok(matched);
        }
        if equivalent_matches > 1 {
            return Err(serde::de::Error::custom(concat!(
                "ambiguous oneOf value for ",
                stringify!(Fired),
                ": more than one branch preserved an equivalent input",
            )));
        }
        equivalent.ok_or_else(|| {
            serde::de::Error::custom(concat!(
                "no oneOf branch for ",
                stringify!(Fired),
                " preserved the complete input",
            ))
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FiredOpened {
    pub event: uuid::Uuid,
    pub outcome: serde_json::Value,
    pub session: uuid::Uuid,
    pub workspace: uuid::Uuid,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FiredIgnored {
    pub correlation: String,
    pub event: uuid::Uuid,
    pub outcome: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FiredFed {
    pub event: uuid::Uuid,
    pub outcome: serde_json::Value,
    pub session: Option<uuid::Uuid>,
    pub workspace: uuid::Uuid,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileListing {
    pub entries: Vec<FileEntry>,
    ///The directory listed, or empty for the Workspace's repositories.
    pub path: String,
    ///How many entries the directory holds, including any not listed.
    ///Constraint: minimum=0
    pub total: i64,
    pub truncated: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileEntry {
    ///Absent for a repository in the Workspace's list of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<FileEntryGit>,
    pub kind: FileEntryKind,
    pub name: String,
    ///Bytes, for a file.
    ///Constraint: minimum=0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum FileEntryKind {
    #[default]
    #[serde(rename = "file")]
    File,
    #[serde(rename = "directory")]
    Directory,
    #[serde(rename = "symlink")]
    Symlink,
    #[serde(rename = "other")]
    Other,
}
impl FileEntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }
}
impl ::std::fmt::Display for FileEntryKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for FileEntryKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum FileEntryGit {
    #[default]
    #[serde(rename = "tracked")]
    Tracked,
    #[serde(rename = "untracked")]
    Untracked,
    #[serde(rename = "ignored")]
    Ignored,
}
impl FileEntryGit {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tracked => "tracked",
            Self::Untracked => "untracked",
            Self::Ignored => "ignored",
        }
    }
}
impl ::std::fmt::Display for FileEntryGit {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for FileEntryGit {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///An Event as kestrel recorded it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventRecord {
    pub event: CloudEvent,
    ///What each Trigger that matched it did, oldest first.
    pub firings: Vec<Firing>,
    ///The Integration that recorded it. Null for an Event kestrel minted itself.
    pub integration: Option<uuid::Uuid>,
    pub organization: uuid::Uuid,
    ///kestrel's own identifier for the record.
    pub record: uuid::Uuid,
    pub recorded_at: String,
}
///One Trigger matching the Event, and what that match did.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Firing {
    ///Why a failed firing started nothing, such as labels choosing two Agents.
    pub failure: Option<String>,
    pub outcome: FiringOutcome,
    pub trigger: String,
    ///The Workspace it opened or fed.
    pub workspace: Option<uuid::Uuid>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum FiringOutcome {
    #[default]
    #[serde(rename = "opened")]
    Opened,
    #[serde(rename = "fed")]
    Fed,
    #[serde(rename = "ignored")]
    Ignored,
    #[serde(rename = "failed")]
    Failed,
}
impl FiringOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Fed => "fed",
            Self::Ignored => "ignored",
            Self::Failed => "failed",
        }
    }
}
impl ::std::fmt::Display for FiringOutcome {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for FiringOutcome {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///The Event's CloudEvents envelope and payload.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CloudEvent {
    pub data: serde_json::Value,
    pub id: String,
    pub source: String,
    pub specversion: String,
    pub subject: Option<String>,
    pub time: String,
    pub r#type: String,
}
///SSE names: entry (Recorded), activity (Activity), session_state (TranscriptSessionState), cursor (CursorEvent), end (End), follower (FollowerEvent), or presence (Presence). Entry, Activity and cursor IDs advance the global Transcript cursor; session_state, follower and presence carry no id. Activity replacements retain first_seq, and their final closed replacement precedes the closing shared-state entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Event {
    Activity(Activity),
    TranscriptSessionState(TranscriptSessionState),
    Recorded(Recorded),
    End(End),
    CursorEvent(CursorEvent),
    FollowerEvent(FollowerEvent),
    Presence(Presence),
}
///Transient session_state SSE snapshot on every follow connect, then changes, including a change of observation availability. Never has an event id and never enters the Transcript. session_id is null when no unfinished Session exists, and observation is then unavailable; usage is the latest the Harness reported, live, or null.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranscriptSessionState {
    ///When the agent last showed activity, present only while its Session trails its answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<String>,
    pub message_buffering: bool,
    pub observation: SessionObservation,
    pub session_id: Option<uuid::Uuid>,
    pub thought_buffering: bool,
    pub tools: Vec<RunningTool>,
    pub units: Vec<RunningUnit>,
    ///What the Harness has spent so far, held in memory beside the running tools while the Session is live and carried to followers; null until it reports one.
    pub usage: Option<Usage>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Usage {
    ///Constraint: minimum=0
    pub context_size: i64,
    ///Constraint: minimum=0
    pub context_used: i64,
    pub cost: Option<Cost>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Cost {
    pub amount: f64,
    pub currency: String,
}
///Whether the Session's open work is currently observed. current: its supervisor sent a snapshot over its open link and is still reaching it; tools and units list that snapshot, empty when nothing is open. unavailable: nothing current is known, whatever the Session's state or lease; tools and units are empty and never mean nothing is open. Neither availability changes the Session's state or scheduling.
#[derive(Debug, Clone)]
pub enum SessionObservation {
    SessionObservationCurrent(SessionObservationCurrent),
    SessionObservationUnavailable(SessionObservationUnavailable),
}
impl serde::Serialize for SessionObservation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::SessionObservationCurrent(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(SessionObservationCurrent),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("availability") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "current") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "availability",
                            stringify!(SessionObservationCurrent),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "availability",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "availability".to_string(),
                            serde_json::Value::String("current".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::SessionObservationUnavailable(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(SessionObservationUnavailable),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("availability") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "unavailable") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "availability",
                            stringify!(SessionObservationUnavailable),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "availability",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "availability".to_string(),
                            serde_json::Value::String("unavailable".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for SessionObservation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("availability") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "availability",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "current" => {
                    let primary_error =
                        match serde_json::from_value::<SessionObservationCurrent>(value.clone()) {
                            Ok(payload) => {
                                return Ok(Self::SessionObservationCurrent(payload));
                            }
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) =
                        serde_json::from_value::<SessionObservationUnavailable>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "availability",
                                "current",
                                first_name,
                                stringify!(SessionObservationUnavailable),
                            )));
                        }
                        structural_match = Some((
                            Self::SessionObservationUnavailable(payload),
                            stringify!(SessionObservationUnavailable),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "unavailable" => {
                    let primary_error = match serde_json::from_value::<SessionObservationUnavailable>(
                        value.clone(),
                    ) {
                        Ok(payload) => {
                            return Ok(Self::SessionObservationUnavailable(payload));
                        }
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) =
                        serde_json::from_value::<SessionObservationCurrent>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "availability",
                                "unavailable",
                                first_name,
                                stringify!(SessionObservationCurrent),
                            )));
                        }
                        structural_match = Some((
                            Self::SessionObservationCurrent(payload),
                            stringify!(SessionObservationCurrent),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "availability",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "availability",
                "`",
            ))),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionObservationUnavailable {
    pub availability: serde_json::Value,
    ///The last snapshot this control plane took for the Session, kept until the Session ends; null when it holds none, as after a restart.
    pub last: Option<LastObservation>,
}
///A historical snapshot of the Session's open work: what was open at observed_at, not proof that it still runs or that the supervisor is reachable.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LastObservation {
    pub observed_at: String,
    pub tools: Vec<RunningTool>,
    pub units: Vec<RunningUnit>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionObservationCurrent {
    pub availability: serde_json::Value,
    ///When the control plane took the snapshot.
    pub observed_at: String,
}
///Work an adapter declared it runs apart from any tool call, such as a Claude background task, open until the adapter settles it. A settled unit adds no Transcript entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunningUnit {
    pub id: String,
    pub kind: RunningUnitKind,
    pub started_at: String,
    pub title: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum RunningUnitKind {
    #[default]
    #[serde(rename = "background_task")]
    BackgroundTask,
    #[serde(rename = "subagent")]
    Subagent,
}
impl RunningUnitKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BackgroundTask => "background_task",
            Self::Subagent => "subagent",
        }
    }
}
impl ::std::fmt::Display for RunningUnitKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for RunningUnitKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunningTool {
    pub call_id: String,
    pub started_at: String,
    pub status: String,
    pub title: String,
    pub tool_kind: String,
}
///One Transcript entry, and where it sits in the order.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recorded {
    pub appended_at: String,
    pub entry: Entry,
    pub kind: TranscriptKind,
    ///Constraint: minimum=1
    pub seq: i64,
    pub session_id: Option<uuid::Uuid>,
}
///The whole current set of a Workspace's followers, sent when a follow registers and whenever the set changes. A named follow appears once per name, the follower's own included; unnamed follows are counted; an Agent never appears. Carries no Transcript cursor.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Presence {
    ///Constraint: minimum=0
    pub anonymous: i64,
    pub named: Vec<String>,
}
///A follow that stayed open past caught-up registered: the id it renews by, and the lease in seconds it must renew before. Sent once per follow.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FollowerEvent {
    pub id: uuid::Uuid,
    ///Constraint: minimum=1
    pub lease_seconds: i64,
}
///A cursor event carries the highest examined workspace:seq as its SSE id and data, so a filtered read resumes beyond omitted entries.
pub type CursorEvent = String;
///One completed unit of the Workspace Transcript.
#[derive(Debug, Clone)]
pub enum Entry {
    ParticipantJoined(ParticipantJoined),
    Brief(Brief),
    SessionStarted(SessionStarted),
    Message(Message),
    Messages(Messages),
    SessionEnded(SessionEnded),
    TurnInterrupted(TurnInterrupted),
    InstanceReleased(InstanceReleased),
    ThoughtEntry(ThoughtEntry),
    PlanEntry(Box<PlanEntry>),
    PullRequestEntry(PullRequestEntry),
    OptionChangedEntry(OptionChangedEntry),
    ToolCallEntry(ToolCallEntry),
    ExpiredEntry(ExpiredEntry),
}
impl serde::Serialize for Entry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::ParticipantJoined(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(ParticipantJoined),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "participant_joined") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(ParticipantJoined),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("participant_joined".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::Brief(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(Brief),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "brief") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(Brief),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("brief".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::SessionStarted(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(SessionStarted),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "session_started") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(SessionStarted),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("session_started".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::Message(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(Message),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "said") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(Message),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("said".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::Messages(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(Messages),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "messages") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(Messages),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("messages".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::SessionEnded(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(SessionEnded),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "session_ended") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(SessionEnded),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("session_ended".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::TurnInterrupted(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(TurnInterrupted),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "turn_interrupted") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(TurnInterrupted),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("turn_interrupted".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::InstanceReleased(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(InstanceReleased),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "instance_released") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(InstanceReleased),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("instance_released".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::ThoughtEntry(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(ThoughtEntry),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "thought") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(ThoughtEntry),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("thought".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::PlanEntry(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(PlanEntry),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "plan") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(PlanEntry),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("plan".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::PullRequestEntry(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(PullRequestEntry),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "pull_request") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(PullRequestEntry),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("pull_request".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::OptionChangedEntry(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(OptionChangedEntry),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag))
                        if matches!(tag.as_str(), "option_changed") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(OptionChangedEntry),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("option_changed".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::ToolCallEntry(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(ToolCallEntry),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "tool_call") => {
                    }
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(ToolCallEntry),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("tool_call".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::ExpiredEntry(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(ExpiredEntry),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("type") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "expired") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "type",
                            stringify!(ExpiredEntry),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "type",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "type".to_string(),
                            serde_json::Value::String("expired".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for Entry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("type") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "type",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "participant_joined" => {
                    let primary_error =
                        match serde_json::from_value::<ParticipantJoined>(value.clone()) {
                            Ok(payload) => return Ok(Self::ParticipantJoined(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "participant_joined",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "brief" => {
                    let primary_error = match serde_json::from_value::<Brief>(value.clone()) {
                        Ok(payload) => return Ok(Self::Brief(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "brief",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "session_started" => {
                    let primary_error =
                        match serde_json::from_value::<SessionStarted>(value.clone()) {
                            Ok(payload) => return Ok(Self::SessionStarted(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_started",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "said" => {
                    let primary_error = match serde_json::from_value::<Message>(value.clone()) {
                        Ok(payload) => return Ok(Self::Message(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "said",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "messages" => {
                    let primary_error = match serde_json::from_value::<Messages>(value.clone()) {
                        Ok(payload) => return Ok(Self::Messages(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "messages",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "session_ended" => {
                    let primary_error = match serde_json::from_value::<SessionEnded>(value.clone())
                    {
                        Ok(payload) => return Ok(Self::SessionEnded(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "session_ended",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "turn_interrupted" => {
                    let primary_error =
                        match serde_json::from_value::<TurnInterrupted>(value.clone()) {
                            Ok(payload) => return Ok(Self::TurnInterrupted(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "turn_interrupted",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "instance_released" => {
                    let primary_error =
                        match serde_json::from_value::<InstanceReleased>(value.clone()) {
                            Ok(payload) => return Ok(Self::InstanceReleased(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "instance_released",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "thought" => {
                    let primary_error = match serde_json::from_value::<ThoughtEntry>(value.clone())
                    {
                        Ok(payload) => return Ok(Self::ThoughtEntry(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "thought",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "plan" => {
                    let primary_error =
                        match serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                            Ok(payload) => return Ok(Self::PlanEntry(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "plan",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "pull_request" => {
                    let primary_error =
                        match serde_json::from_value::<PullRequestEntry>(value.clone()) {
                            Ok(payload) => return Ok(Self::PullRequestEntry(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "pull_request",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "option_changed" => {
                    let primary_error =
                        match serde_json::from_value::<OptionChangedEntry>(value.clone()) {
                            Ok(payload) => return Ok(Self::OptionChangedEntry(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "option_changed",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "tool_call" => {
                    let primary_error = match serde_json::from_value::<ToolCallEntry>(value.clone())
                    {
                        Ok(payload) => return Ok(Self::ToolCallEntry(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ExpiredEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "tool_call",
                                first_name,
                                stringify!(ExpiredEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ExpiredEntry(payload), stringify!(ExpiredEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "expired" => {
                    let primary_error = match serde_json::from_value::<ExpiredEntry>(value.clone())
                    {
                        Ok(payload) => return Ok(Self::ExpiredEntry(payload)),
                        Err(error) => error,
                    };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) = serde_json::from_value::<ParticipantJoined>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(ParticipantJoined),
                            )));
                        }
                        structural_match = Some((
                            Self::ParticipantJoined(payload),
                            stringify!(ParticipantJoined),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<Brief>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(Brief),
                            )));
                        }
                        structural_match = Some((Self::Brief(payload), stringify!(Brief)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionStarted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(SessionStarted),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionStarted(payload), stringify!(SessionStarted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Message>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(Message),
                            )));
                        }
                        structural_match = Some((Self::Message(payload), stringify!(Message)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Messages>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(Messages),
                            )));
                        }
                        structural_match = Some((Self::Messages(payload), stringify!(Messages)));
                    }
                    if let Ok(payload) = serde_json::from_value::<SessionEnded>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(SessionEnded),
                            )));
                        }
                        structural_match =
                            Some((Self::SessionEnded(payload), stringify!(SessionEnded)));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnInterrupted>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(TurnInterrupted),
                            )));
                        }
                        structural_match =
                            Some((Self::TurnInterrupted(payload), stringify!(TurnInterrupted)));
                    }
                    if let Ok(payload) = serde_json::from_value::<InstanceReleased>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(InstanceReleased),
                            )));
                        }
                        structural_match = Some((
                            Self::InstanceReleased(payload),
                            stringify!(InstanceReleased),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ThoughtEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(ThoughtEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ThoughtEntry(payload), stringify!(ThoughtEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<Box<PlanEntry>>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(PlanEntry),
                            )));
                        }
                        structural_match = Some((Self::PlanEntry(payload), stringify!(PlanEntry)));
                    }
                    if let Ok(payload) = serde_json::from_value::<PullRequestEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(PullRequestEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::PullRequestEntry(payload),
                            stringify!(PullRequestEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<OptionChangedEntry>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(OptionChangedEntry),
                            )));
                        }
                        structural_match = Some((
                            Self::OptionChangedEntry(payload),
                            stringify!(OptionChangedEntry),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<ToolCallEntry>(value.clone()) {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "type",
                                "expired",
                                first_name,
                                stringify!(ToolCallEntry),
                            )));
                        }
                        structural_match =
                            Some((Self::ToolCallEntry(payload), stringify!(ToolCallEntry)));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "type",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "type",
                "`",
            ))),
        }
    }
}
///A person interrupted a working Turn without ending the Session.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TurnInterrupted {
    pub participant: String,
    pub session: uuid::Uuid,
    pub r#type: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCallEntry {
    pub call_id: String,
    ///Why kestrel closed the call at a Turn boundary; null when the Harness settled it.
    pub closing_reason: Option<ToolCallEntryClosingReason>,
    pub completion: Completion,
    ///Inline JSON, or a PayloadReference when the serialized value exceeds 65536 bytes.
    pub input: serde_json::Value,
    ///Fields containing payload references; absent means every content field is inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_fields: Option<Vec<String>>,
    ///Inline JSON, or a PayloadReference when the serialized value exceeds 65536 bytes.
    pub result: serde_json::Value,
    pub session_id: uuid::Uuid,
    ///The status the Harness last reported for the call.
    pub status: ToolCallEntryStatus,
    pub title: String,
    pub tool_kind: String,
    pub r#type: serde_json::Value,
}
///The status the Harness last reported for the call.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum ToolCallEntryStatus {
    #[default]
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
}
impl ToolCallEntryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}
impl ::std::fmt::Display for ToolCallEntryStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for ToolCallEntryStatus {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///Why kestrel closed the call at a Turn boundary; null when the Harness settled it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum ToolCallEntryClosingReason {
    #[default]
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "unresolved")]
    Unresolved,
    #[serde(rename = "null")]
    NullValue,
}
impl ToolCallEntryClosingReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Unresolved => "unresolved",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for ToolCallEntryClosingReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for ToolCallEntryClosingReason {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThoughtEntry {
    pub completion: Completion,
    ///Fields containing payload references; absent means every content field is inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_fields: Option<Vec<String>>,
    pub session_id: uuid::Uuid,
    pub text: ThoughtContent,
    pub r#type: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ThoughtContent {
    InlineThought(InlineThought),
    PayloadReference(PayloadReference),
}
pub type InlineThought = String;
///A Session started on behalf of the Workspace.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionStarted {
    ///The Agent the Session runs.
    pub agent: String,
    pub session: uuid::Uuid,
    pub r#type: serde_json::Value,
}
///A Session ended, and how it went.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionEnded {
    pub exit: Exit,
    pub session: uuid::Uuid,
    pub r#type: serde_json::Value,
}
///How a Session ended.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Exit {
    ///Why it failed. Absent when it succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub because: Option<String>,
    pub status: ExitStatus,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum ExitStatus {
    #[default]
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
}
impl ExitStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}
impl ::std::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for ExitStatus {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///A verified GitHub Event observed a pull request from the Workspace's declared branch in one of its repositories. Learning it starts no work.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PullRequestEntry {
    ///GitHub's action, such as opened.
    pub action: String,
    ///The Organization Event it was learned from.
    pub event: uuid::Uuid,
    ///Constraint: minimum=1
    pub number: i64,
    ///The head repository fixed on the Workspace.
    pub repository: String,
    pub state: PullRequestState,
    pub title: String,
    pub r#type: serde_json::Value,
    pub url: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum PullRequestState {
    #[default]
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "closed")]
    Closed,
    #[serde(rename = "merged")]
    Merged,
}
impl PullRequestState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Merged => "merged",
        }
    }
}
impl ::std::fmt::Display for PullRequestState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for PullRequestState {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlanEntry {
    pub completion: Completion,
    pub entries: Box<PlanEntries>,
    ///Fields containing payload references; absent means every content field is inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_fields: Option<Vec<String>>,
    pub session_id: uuid::Uuid,
    pub r#type: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PlanEntries {
    InlinePlanEntries(Box<InlinePlanEntries>),
    PayloadReference(PayloadReference),
}
pub type InlinePlanEntries = Vec<Box<PlanEntry>>;
///A Participant joined the Workspace.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParticipantJoined {
    pub participant: String,
    pub r#type: serde_json::Value,
}
///A person changed one of a Session's options between Turns (ADR-0041).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OptionChangedEntry {
    pub category: String,
    ///What the option was set to before the change.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub from: Option<Option<String>>,
    ///The harness option's id, or the category when a queued Session's declared value changed.
    pub option: String,
    ///The person who made the change.
    pub participant: String,
    ///Why the harness refused, when it did.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub refused: Option<Option<String>>,
    pub session: uuid::Uuid,
    ///What it was set to; null when the harness refused.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub to: Option<Option<String>>,
    pub r#type: serde_json::Value,
}
///Messages that arrived while a Session was active, drained as one Transcript entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Messages {
    pub messages: MessageCollection,
    ///Fields containing payload references; absent means every content field is inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_fields: Option<Vec<String>>,
    pub r#type: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageCollection {
    InlineMessages(InlineMessages),
    PayloadReference(PayloadReference),
}
pub type InlineMessages = Vec<SpokenMessage>;
///One Participant's message within a drained group.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpokenMessage {
    pub message: MessageContent,
    pub participant: String,
    ///Fields containing payload references; absent means every content field is inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_fields: Option<Vec<String>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    InlineMessage(InlineMessage),
    PayloadReference(PayloadReference),
}
pub type InlineMessage = String;
///What a Participant said.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<Completion>,
    pub message: String,
    pub participant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<uuid::Uuid>,
    pub r#type: serde_json::Value,
}
///A person released the Workspace's Instance, destroying it and whatever it held that was never pushed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstanceReleased {
    pub instance: String,
    pub participant: String,
    pub r#type: serde_json::Value,
    ///What it held that existed nowhere else, as judged when it was released; null when it held nothing.
    pub unpublished: Option<String>,
}
///Narration or detail expired in place after 30 days from append, including in sealed Workspaces. The enclosing record retains its original kind, seq and appended_at, with session_id null; all content, completion metadata and payload references are removed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpiredEntry {
    ///When the sweep replaced the entry.
    pub expired_at: String,
    pub r#type: serde_json::Value,
}
///The control plane closed the stream on purpose. A stream that closes without one was cut off, and is resumed from the last id delivered.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct End {
    ///`caught_up` when a read that does not follow has delivered every entry; `sealed` when the Workspace is sealed and every entry it will ever hold has been delivered.
    pub because: EndBecause,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum EndBecause {
    #[default]
    #[serde(rename = "caught_up")]
    CaughtUp,
    #[serde(rename = "sealed")]
    Sealed,
}
impl EndBecause {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CaughtUp => "caught_up",
            Self::Sealed => "sealed",
        }
    }
}
impl ::std::fmt::Display for EndBecause {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for EndBecause {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///A typed Diagnostic, or the plain Refusal a refusal falls back to until its producer is typed. Only `kind` tells them apart.
#[derive(Debug, Clone)]
pub enum DiagnosedRefusal {
    Diagnostic(Diagnostic),
    Refusal(Refusal),
}
impl Serialize for DiagnosedRefusal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Diagnostic(value) => serde::Serialize::serialize(value, serializer),
            Self::Refusal(value) => serde::Serialize::serialize(value, serializer),
        }
    }
}
impl<'de> Deserialize<'de> for DiagnosedRefusal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn exact_json_integer(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
        }
        fn json_numbers_have_same_value(
            encoded: &serde_json::Number,
            input: &serde_json::Number,
        ) -> bool {
            match (exact_json_integer(encoded), exact_json_integer(input)) {
                (Some(encoded), Some(input)) => encoded == input,
                (Some(encoded), None) => input.as_f64().is_some_and(|input| {
                    input.is_finite() && input.fract() == 0.0 && input as i128 == encoded
                }),
                (None, Some(input)) => encoded.as_f64().is_some_and(|encoded| {
                    encoded.is_finite() && encoded.fract() == 0.0 && encoded as i128 == input
                }),
                (None, None) => encoded.as_f64() == input.as_f64(),
            }
        }
        /// `nulls_may_be_absent` also accepts an input `null` that the
        /// branch omits, as a skipped `None` does. Extra encoded
        /// keys are allowed only by the pre-existing anyOf match.
        fn preserves_complete_json_input(
            encoded: &serde_json::Value,
            input: &serde_json::Value,
            nulls_may_be_absent: bool,
            encoded_keys_may_be_extra: bool,
        ) -> bool {
            match (encoded, input) {
                (serde_json::Value::Object(encoded), serde_json::Value::Object(input)) => {
                    (encoded_keys_may_be_extra
                        || encoded.iter().all(|(key, value)| {
                            input.contains_key(key) || (nulls_may_be_absent && value.is_null())
                        }))
                        && input.iter().all(|(key, value)| match encoded.get(key) {
                            Some(encoded_value) => preserves_complete_json_input(
                                encoded_value,
                                value,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            ),
                            None => nulls_may_be_absent && value.is_null(),
                        })
                }
                (serde_json::Value::Array(encoded), serde_json::Value::Array(input)) => {
                    encoded.len() == input.len()
                        && encoded.iter().zip(input).all(|(encoded, input)| {
                            preserves_complete_json_input(
                                encoded,
                                input,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            )
                        })
                }
                (serde_json::Value::Number(encoded), serde_json::Value::Number(input)) => {
                    json_numbers_have_same_value(encoded, input)
                }
                _ => encoded == input,
            }
        }
        let input = <serde_json::Value as Deserialize>::deserialize(deserializer)?;
        let mut equivalent = None;
        if let Ok(candidate) = serde_json::from_value::<Diagnostic>(input.clone()) {
            match serde_json::to_value(&candidate) {
                Ok(encoded) if preserves_complete_json_input(&encoded, &input, false, true) => {
                    return Ok(Self::Diagnostic(candidate));
                }
                Ok(encoded)
                    if equivalent.is_none()
                        && preserves_complete_json_input(&encoded, &input, true, false) =>
                {
                    equivalent = Some(Self::Diagnostic(candidate));
                }
                _ => {}
            }
        }
        if let Ok(candidate) = serde_json::from_value::<Refusal>(input.clone()) {
            match serde_json::to_value(&candidate) {
                Ok(encoded) if preserves_complete_json_input(&encoded, &input, false, true) => {
                    return Ok(Self::Refusal(candidate));
                }
                Ok(encoded)
                    if equivalent.is_none()
                        && preserves_complete_json_input(&encoded, &input, true, false) =>
                {
                    equivalent = Some(Self::Refusal(candidate));
                }
                _ => {}
            }
        }
        equivalent.ok_or_else(|| {
            serde::de::Error::custom(concat!(
                "no anyOf branch for ",
                stringify!(DiagnosedRefusal),
                " preserved the complete input",
            ))
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Refusal {
    ///The request field the message concerns, when the refusal is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}
///Why kestrel would not do what it was asked, and the ordered, typed next steps that repair, inspect or retry it (ADR-0052). `message` is display only; a Client never classifies it.
#[derive(Debug, Clone)]
pub enum Diagnostic {
    MissingReferenceDiagnostic(MissingReferenceDiagnostic),
    AmbiguousReferenceDiagnostic(AmbiguousReferenceDiagnostic),
    MalformedRequestDiagnostic(MalformedRequestDiagnostic),
    ForbiddenActionDiagnostic(ForbiddenActionDiagnostic),
    StateConflictDiagnostic(StateConflictDiagnostic),
    ExpiredResourceDiagnostic(ExpiredResourceDiagnostic),
    InvalidFieldDiagnostic(InvalidFieldDiagnostic),
    SetupGapDiagnostic(SetupGapDiagnostic),
    UnavailableDiagnostic(UnavailableDiagnostic),
    InstanceTimeoutDiagnostic(InstanceTimeoutDiagnostic),
    AuthenticationFailedDiagnostic(AuthenticationFailedDiagnostic),
    ExecutableMissingDiagnostic(ExecutableMissingDiagnostic),
    UnknownFailureDiagnostic(UnknownFailureDiagnostic),
    ConnectionFailedDiagnostic(ConnectionFailedDiagnostic),
    ClientFailureDiagnostic(ClientFailureDiagnostic),
    UnknownResponseDiagnostic(UnknownResponseDiagnostic),
}
impl Serialize for Diagnostic {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::MissingReferenceDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::AmbiguousReferenceDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::MalformedRequestDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::ForbiddenActionDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::StateConflictDiagnostic(value) => serde::Serialize::serialize(value, serializer),
            Self::ExpiredResourceDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::InvalidFieldDiagnostic(value) => serde::Serialize::serialize(value, serializer),
            Self::SetupGapDiagnostic(value) => serde::Serialize::serialize(value, serializer),
            Self::UnavailableDiagnostic(value) => serde::Serialize::serialize(value, serializer),
            Self::InstanceTimeoutDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::AuthenticationFailedDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::ExecutableMissingDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::UnknownFailureDiagnostic(value) => serde::Serialize::serialize(value, serializer),
            Self::ConnectionFailedDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::ClientFailureDiagnostic(value) => serde::Serialize::serialize(value, serializer),
            Self::UnknownResponseDiagnostic(value) => {
                serde::Serialize::serialize(value, serializer)
            }
        }
    }
}
impl<'de> Deserialize<'de> for Diagnostic {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn exact_json_integer(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
        }
        fn json_numbers_have_same_value(
            encoded: &serde_json::Number,
            input: &serde_json::Number,
        ) -> bool {
            match (exact_json_integer(encoded), exact_json_integer(input)) {
                (Some(encoded), Some(input)) => encoded == input,
                (Some(encoded), None) => input.as_f64().is_some_and(|input| {
                    input.is_finite() && input.fract() == 0.0 && input as i128 == encoded
                }),
                (None, Some(input)) => encoded.as_f64().is_some_and(|encoded| {
                    encoded.is_finite() && encoded.fract() == 0.0 && encoded as i128 == input
                }),
                (None, None) => encoded.as_f64() == input.as_f64(),
            }
        }
        /// `nulls_may_be_absent` also accepts an input `null` that the
        /// branch omits, as a skipped `None` does. Extra encoded
        /// keys are allowed only by the pre-existing anyOf match.
        fn preserves_complete_json_input(
            encoded: &serde_json::Value,
            input: &serde_json::Value,
            nulls_may_be_absent: bool,
            encoded_keys_may_be_extra: bool,
        ) -> bool {
            match (encoded, input) {
                (serde_json::Value::Object(encoded), serde_json::Value::Object(input)) => {
                    (encoded_keys_may_be_extra
                        || encoded.iter().all(|(key, value)| {
                            input.contains_key(key) || (nulls_may_be_absent && value.is_null())
                        }))
                        && input.iter().all(|(key, value)| match encoded.get(key) {
                            Some(encoded_value) => preserves_complete_json_input(
                                encoded_value,
                                value,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            ),
                            None => nulls_may_be_absent && value.is_null(),
                        })
                }
                (serde_json::Value::Array(encoded), serde_json::Value::Array(input)) => {
                    encoded.len() == input.len()
                        && encoded.iter().zip(input).all(|(encoded, input)| {
                            preserves_complete_json_input(
                                encoded,
                                input,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            )
                        })
                }
                (serde_json::Value::Number(encoded), serde_json::Value::Number(input)) => {
                    json_numbers_have_same_value(encoded, input)
                }
                _ => encoded == input,
            }
        }
        let input = <serde_json::Value as Deserialize>::deserialize(deserializer)?;
        let mut matched = None;
        let mut equivalent = None;
        let mut equivalent_matches = 0usize;
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"missing_reference\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<MissingReferenceDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::MissingReferenceDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::MissingReferenceDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"ambiguous_reference\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<AmbiguousReferenceDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::AmbiguousReferenceDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::AmbiguousReferenceDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"malformed_request\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<MalformedRequestDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::MalformedRequestDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::MalformedRequestDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"forbidden_action\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<ForbiddenActionDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ForbiddenActionDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ForbiddenActionDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"state_conflict\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<StateConflictDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::StateConflictDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::StateConflictDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"expired_resource\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<ExpiredResourceDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ExpiredResourceDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ExpiredResourceDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"invalid_field\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<InvalidFieldDiagnostic>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::InvalidFieldDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::InvalidFieldDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"setup_gap\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<SetupGapDiagnostic>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::SetupGapDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::SetupGapDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"unavailable\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<UnavailableDiagnostic>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::UnavailableDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::UnavailableDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"instance_timeout\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<InstanceTimeoutDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::InstanceTimeoutDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::InstanceTimeoutDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"authentication_failed\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<AuthenticationFailedDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::AuthenticationFailedDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::AuthenticationFailedDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"executable_missing\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<ExecutableMissingDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ExecutableMissingDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ExecutableMissingDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"unknown_failure\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<UnknownFailureDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::UnknownFailureDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::UnknownFailureDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"connection_failed\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<ConnectionFailedDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ConnectionFailedDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ConnectionFailedDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"client_failure\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<ClientFailureDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ClientFailureDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ClientFailureDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"unknown_response\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<UnknownResponseDiagnostic>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Diagnostic),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::UnknownResponseDiagnostic(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::UnknownResponseDiagnostic(candidate));
                    }
                    _ => {}
                }
            }
        }
        if let Some(matched) = matched {
            return Ok(matched);
        }
        if equivalent_matches > 1 {
            return Err(serde::de::Error::custom(concat!(
                "ambiguous oneOf value for ",
                stringify!(Diagnostic),
                ": more than one branch preserved an equivalent input",
            )));
        }
        equivalent.ok_or_else(|| {
            serde::de::Error::custom(concat!(
                "no oneOf branch for ",
                stringify!(Diagnostic),
                " preserved the complete input",
            ))
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnknownResponseDiagnostic {
    pub context: UnknownResponseContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnknownResponseContext {
    pub evidence: Option<String>,
    pub operation: String,
    pub service: String,
    pub status: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnknownFailureDiagnostic {
    pub context: UnknownFailureContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnknownFailureContext {
    pub evidence: Option<UnknownEvidence>,
    pub resource: Option<String>,
    pub session: Option<String>,
}
///What the agent said, bounded and redacted. It never establishes that a sign-in expired or is not covered.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnknownEvidence {
    pub kind: serde_json::Value,
    ///Constraint: maxLength=1024
    pub summary: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnavailableDiagnostic {
    pub context: UnavailableContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnavailableContext {
    pub operation: String,
    pub resource: Option<String>,
    pub retry_after_seconds: Option<i64>,
    pub service: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateConflictDiagnostic {
    pub context: StateConflictContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateConflictContext {
    ///The Session holding the conflicting state, when the conflict is one.
    pub holding_session: Option<String>,
    pub operation: String,
    pub organization: Option<String>,
    pub reference: String,
    pub resource: Resource,
    pub state: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetupGapDiagnostic {
    pub context: SetupGapContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetupGapContext {
    pub harness: Option<String>,
    pub image: Option<String>,
    pub method: Option<String>,
    pub organization: Option<String>,
    pub prerequisite: String,
    pub reference: Option<String>,
    pub resource: Option<Resource>,
    pub sign_in: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MissingReferenceDiagnostic {
    pub context: MissingReferenceContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MissingReferenceContext {
    ///Null when the resource is not scoped to an Organization.
    pub organization: Option<String>,
    pub reference: String,
    pub resource: Resource,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MalformedRequestDiagnostic {
    pub context: MalformedRequestContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MalformedRequestContext {
    pub field: Option<String>,
    pub operation: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InvalidFieldDiagnostic {
    pub context: InvalidFieldContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InvalidFieldContext {
    pub allowed_values: Option<Vec<String>>,
    pub constraint: String,
    pub field: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstanceTimeoutDiagnostic {
    pub context: InstanceTimeoutContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstanceTimeoutContext {
    pub instance: Option<String>,
    pub operation: String,
    pub workspace: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ForbiddenActionDiagnostic {
    pub context: ForbiddenActionContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ForbiddenActionContext {
    ///The existing author/name constraint the action violates, when established.
    pub constraint: Option<String>,
    pub operation: String,
    pub organization: Option<String>,
    pub reference: String,
    pub resource: Resource,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpiredResourceDiagnostic {
    pub context: ExpiredResourceContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpiredResourceContext {
    pub operation: String,
    pub organization: Option<String>,
    pub reference: String,
    pub resource: Resource,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutableMissingDiagnostic {
    pub context: ExecutableMissingContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutableMissingContext {
    pub evidence: ExecutableMissingEvidence,
    pub executable: String,
    pub harness: String,
    pub image: Option<String>,
    pub session: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutableMissingEvidence {
    pub command: String,
    pub error: OsError,
    pub kind: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsError {
    pub code: Option<i64>,
    pub kind: OsErrorKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum OsErrorKind {
    #[default]
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "permission_denied")]
    PermissionDenied,
    #[serde(rename = "other")]
    Other,
}
impl OsErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::Other => "other",
        }
    }
}
impl ::std::fmt::Display for OsErrorKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for OsErrorKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclarationDocument {
    pub agent: AgentDeclaration,
    pub project: ProjectDeclaration,
    pub trigger: DeclarationDocumentTrigger,
}
#[derive(Debug, Clone)]
pub struct TriggerDeclaration {
    ///Constraint: minLength=1
    pub agent: String,
    pub allows: Option<Vec<String>>,
    pub branch: Option<Option<String>>,
    ///Constraint: minLength=1
    pub brief: String,
    pub correlation: Option<Option<String>>,
    ///Five fields of minute, hour, day of the month, month and day of the week, such as `0 9 * * 1-5`, in place of a filter or an interval.
    pub cron: Option<String>,
    ///A duration such as `1h`, counted from the declaration, in place of a filter.
    pub every: Option<String>,
    ///A CloudEvents filter of exact, prefix, suffix, all, any, or not.
    pub filter: Option<serde_json::Value>,
    ///The mode its Sessions run in. Omitted, the Agent's, then the Harness's default.
    pub mode: Option<Option<String>>,
    ///The model its Sessions run with. Omitted, the Agent's, then the Harness's default.
    pub model: Option<Option<String>>,
    ///Constraint: minLength=1
    pub name: String,
    pub on_miss: Option<Option<TriggerDeclarationOnMiss>>,
    ///What a firing that correlates to an open Workspace does: continues its waiting Session, or starts a new Session with this Trigger's Agent. Absent continues.
    pub on_open_workspace: Option<Option<TriggerDeclarationOnOpenWorkspace>>,
    pub profile: Option<Option<String>>,
    ///Constraint: minLength=1
    pub project: String,
    ///The thought level its Sessions run at. Omitted, the Agent's, then the Harness's default.
    pub thought_level: Option<Option<String>>,
    ///The IANA time zone a cron expression is read in, such as `America/New_York`.
    pub zone: Option<String>,
    /// The variant this value takes, alongside the fields above.
    pub variant: TriggerDeclarationVariant,
}
#[derive(Deserialize, Serialize)]
struct __TriggerDeclarationBase {
    agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    allows: Option<Vec<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    branch: Option<Option<String>>,
    brief: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    correlation: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cron: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    every: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<serde_json::Value>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    mode: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    model: Option<Option<String>>,
    name: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    on_miss: Option<Option<TriggerDeclarationOnMiss>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    on_open_workspace: Option<Option<TriggerDeclarationOnOpenWorkspace>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    profile: Option<Option<String>>,
    project: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    thought_level: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone: Option<String>,
}
impl serde::Serialize for TriggerDeclaration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let base = __TriggerDeclarationBase {
            agent: self.agent.clone(),
            allows: self.allows.clone(),
            branch: self.branch.clone(),
            brief: self.brief.clone(),
            correlation: self.correlation.clone(),
            cron: self.cron.clone(),
            every: self.every.clone(),
            filter: self.filter.clone(),
            mode: self.mode.clone(),
            model: self.model.clone(),
            name: self.name.clone(),
            on_miss: self.on_miss.clone(),
            on_open_workspace: self.on_open_workspace.clone(),
            profile: self.profile.clone(),
            project: self.project.clone(),
            thought_level: self.thought_level.clone(),
            zone: self.zone.clone(),
        };
        let mut value = serde_json::to_value(base).map_err(serde::ser::Error::custom)?;
        let variant = serde_json::to_value(&self.variant).map_err(serde::ser::Error::custom)?;
        let object = value.as_object_mut().ok_or_else(|| {
            serde::ser::Error::custom("shared union base did not serialize as an object")
        })?;
        let variant_object = variant.as_object().ok_or_else(|| {
            serde::ser::Error::custom("shared union variant did not serialize as an object")
        })?;
        for (key, variant_value) in variant_object {
            if let Some(base_value) = object.get(key)
                && base_value != variant_value
            {
                return Err(serde::ser::Error::custom(format!(
                    "shared union field `{key}` serialized conflicting values",
                )));
            }
            object.insert(key.clone(), variant_value.clone());
        }
        serde::Serialize::serialize(&value, serializer)
    }
}
impl<'de> serde::Deserialize<'de> for TriggerDeclaration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let base = serde_json::from_value::<__TriggerDeclarationBase>(value.clone())
            .map_err(serde::de::Error::custom)?;
        let variant = match serde_json::from_value::<TriggerDeclarationVariant>(value.clone()) {
            Ok(variant) => variant,
            Err(complete_error) => {
                let mut variant_input = value;
                if let Some(object) = variant_input.as_object_mut() {
                    object.remove("agent");
                    object.remove("allows");
                    object.remove("branch");
                    object.remove("brief");
                    object.remove("correlation");
                    object.remove("cron");
                    object.remove("every");
                    object.remove("filter");
                    object.remove("mode");
                    object.remove("model");
                    object.remove("name");
                    object.remove("on_miss");
                    object.remove("on_open_workspace");
                    object.remove("profile");
                    object.remove("project");
                    object.remove("thought_level");
                    object.remove("zone");
                }
                serde_json::from_value::<TriggerDeclarationVariant>(variant_input)
                    .map_err(|projected_error| serde::de::Error::custom(
                        format!(
                            "complete shared-union input failed: {complete_error}; projected input failed: {projected_error}",
                        ),
                    ))?
            }
        };
        Ok(Self {
            agent: base.agent,
            allows: base.allows,
            branch: base.branch,
            brief: base.brief,
            correlation: base.correlation,
            cron: base.cron,
            every: base.every,
            filter: base.filter,
            mode: base.mode,
            model: base.model,
            name: base.name,
            on_miss: base.on_miss,
            on_open_workspace: base.on_open_workspace,
            profile: base.profile,
            project: base.project,
            thought_level: base.thought_level,
            zone: base.zone,
            variant: variant,
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TriggerDeclarationVariant {
    EventTriggerCondition(EventTriggerCondition),
    IntervalTriggerCondition(IntervalTriggerCondition),
    CronTriggerCondition(CronTriggerCondition),
}
pub type IntervalTriggerCondition = serde_json::Value;
pub type EventTriggerCondition = serde_json::Value;
pub type CronTriggerCondition = serde_json::Value;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectDeclaration {
    ///Constraint: minLength=1
    pub branch: String,
    ///Constraint: minLength=1
    pub name: String,
    ///The repositories the work happens against, in order: an http(s)://, ssh://, git:// or file:// URL, an scp-style `user@host:path`, a local path on the Instance beginning `/`, `./` or `../`, or `owner/repo` or `github.com/owner/repo`, which is held as its HTTPS `.git` address.
    ///Constraint: minItems=1
    pub repositories: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct DeclarationDocumentTrigger {
    ///Constraint: minLength=1
    pub agent: String,
    pub allows: Option<Vec<String>>,
    pub branch: Option<Option<String>>,
    ///Constraint: minLength=1
    pub brief: String,
    pub correlation: Option<Option<String>>,
    ///Five fields of minute, hour, day of the month, month and day of the week, such as `0 9 * * 1-5`, in place of a filter or an interval.
    pub cron: Option<String>,
    ///A duration such as `1h`, counted from the declaration, in place of a filter.
    pub every: Option<String>,
    ///A CloudEvents filter of exact, prefix, suffix, all, any, or not.
    pub filter: Option<serde_json::Value>,
    ///The mode its Sessions run in. Omitted, the Agent's, then the Harness's default.
    pub mode: Option<Option<String>>,
    ///The model its Sessions run with. Omitted, the Agent's, then the Harness's default.
    pub model: Option<Option<String>>,
    ///Constraint: minLength=1
    pub name: String,
    pub on_miss: Option<Option<TriggerDeclarationOnMiss>>,
    ///What a firing that correlates to an open Workspace does: continues its waiting Session, or starts a new Session with this Trigger's Agent. Absent continues.
    pub on_open_workspace: Option<Option<TriggerDeclarationOnOpenWorkspace>>,
    pub profile: Option<Option<String>>,
    ///Constraint: minLength=1
    pub project: String,
    ///The thought level its Sessions run at. Omitted, the Agent's, then the Harness's default.
    pub thought_level: Option<Option<String>>,
    ///The IANA time zone a cron expression is read in, such as `America/New_York`.
    pub zone: Option<String>,
    /// The variant this value takes, alongside the fields above.
    pub variant: TriggerDeclarationVariant,
}
#[derive(Deserialize, Serialize)]
struct __DeclarationDocumentTriggerBase {
    agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    allows: Option<Vec<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    branch: Option<Option<String>>,
    brief: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    correlation: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cron: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    every: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<serde_json::Value>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    mode: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    model: Option<Option<String>>,
    name: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    on_miss: Option<Option<TriggerDeclarationOnMiss>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    on_open_workspace: Option<Option<TriggerDeclarationOnOpenWorkspace>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    profile: Option<Option<String>>,
    project: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    thought_level: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zone: Option<String>,
}
impl serde::Serialize for DeclarationDocumentTrigger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let base = __DeclarationDocumentTriggerBase {
            agent: self.agent.clone(),
            allows: self.allows.clone(),
            branch: self.branch.clone(),
            brief: self.brief.clone(),
            correlation: self.correlation.clone(),
            cron: self.cron.clone(),
            every: self.every.clone(),
            filter: self.filter.clone(),
            mode: self.mode.clone(),
            model: self.model.clone(),
            name: self.name.clone(),
            on_miss: self.on_miss.clone(),
            on_open_workspace: self.on_open_workspace.clone(),
            profile: self.profile.clone(),
            project: self.project.clone(),
            thought_level: self.thought_level.clone(),
            zone: self.zone.clone(),
        };
        let mut value = serde_json::to_value(base).map_err(serde::ser::Error::custom)?;
        let variant = serde_json::to_value(&self.variant).map_err(serde::ser::Error::custom)?;
        let object = value.as_object_mut().ok_or_else(|| {
            serde::ser::Error::custom("shared union base did not serialize as an object")
        })?;
        let variant_object = variant.as_object().ok_or_else(|| {
            serde::ser::Error::custom("shared union variant did not serialize as an object")
        })?;
        for (key, variant_value) in variant_object {
            if let Some(base_value) = object.get(key)
                && base_value != variant_value
            {
                return Err(serde::ser::Error::custom(format!(
                    "shared union field `{key}` serialized conflicting values",
                )));
            }
            object.insert(key.clone(), variant_value.clone());
        }
        serde::Serialize::serialize(&value, serializer)
    }
}
impl<'de> serde::Deserialize<'de> for DeclarationDocumentTrigger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let base = serde_json::from_value::<__DeclarationDocumentTriggerBase>(value.clone())
            .map_err(serde::de::Error::custom)?;
        let variant = match serde_json::from_value::<TriggerDeclarationVariant>(value.clone()) {
            Ok(variant) => variant,
            Err(complete_error) => {
                let mut variant_input = value;
                if let Some(object) = variant_input.as_object_mut() {
                    object.remove("agent");
                    object.remove("allows");
                    object.remove("branch");
                    object.remove("brief");
                    object.remove("correlation");
                    object.remove("cron");
                    object.remove("every");
                    object.remove("filter");
                    object.remove("mode");
                    object.remove("model");
                    object.remove("name");
                    object.remove("on_miss");
                    object.remove("on_open_workspace");
                    object.remove("profile");
                    object.remove("project");
                    object.remove("thought_level");
                    object.remove("zone");
                }
                serde_json::from_value::<TriggerDeclarationVariant>(variant_input)
                    .map_err(|projected_error| serde::de::Error::custom(
                        format!(
                            "complete shared-union input failed: {complete_error}; projected input failed: {projected_error}",
                        ),
                    ))?
            }
        };
        Ok(Self {
            agent: base.agent,
            allows: base.allows,
            branch: base.branch,
            brief: base.brief,
            correlation: base.correlation,
            cron: base.cron,
            every: base.every,
            filter: base.filter,
            mode: base.mode,
            model: base.model,
            name: base.name,
            on_miss: base.on_miss,
            on_open_workspace: base.on_open_workspace,
            profile: base.profile,
            project: base.project,
            thought_level: base.thought_level,
            zone: base.zone,
            variant: variant,
        })
    }
}
///What a firing that correlates to an open Workspace does: continues its waiting Session, or starts a new Session with this Trigger's Agent. Absent continues.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerDeclarationOnOpenWorkspace {
    #[default]
    #[serde(rename = "continue")]
    Continue_,
    #[serde(rename = "new-session")]
    NewSession,
    #[serde(rename = "null")]
    NullValue,
}
impl TriggerDeclarationOnOpenWorkspace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Continue_ => "continue",
            Self::NewSession => "new-session",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for TriggerDeclarationOnOpenWorkspace {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerDeclarationOnOpenWorkspace {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TriggerDeclarationOnMiss {
    #[default]
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "ignore")]
    Ignore,
    #[serde(rename = "null")]
    NullValue,
}
impl TriggerDeclarationOnMiss {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Ignore => "ignore",
            Self::NullValue => "null",
        }
    }
}
impl ::std::fmt::Display for TriggerDeclarationOnMiss {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TriggerDeclarationOnMiss {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentDeclaration {
    ///The Harness that drives it.
    ///Constraint: minLength=1
    pub harness: String,
    ///The mode it works in. Absent or null names none, and the Harness's own default is the answer.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub mode: Option<Option<String>>,
    ///The model it works with. Absent or null names none, and the Harness's own default is the answer.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub model: Option<Option<String>>,
    ///Constraint: minLength=1
    pub name: String,
    ///The thought level it works at. Absent or null names none, and the Harness's own default is the answer.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub thought_level: Option<Option<String>>,
}
impl AgentDeclaration {
    /// Construct this request with every required wire field.
    pub fn new(harness: String, name: String) -> Self {
        Self {
            harness,
            name,
            mode: None,
            model: None,
            thought_level: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(harness: String, name: String) -> AgentDeclarationBuilder {
        AgentDeclarationBuilder::new(harness, name)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct AgentDeclarationBuilder {
    value: AgentDeclaration,
}
impl AgentDeclarationBuilder {
    /// Start a builder with every required wire field.
    pub fn new(harness: String, name: String) -> Self {
        Self {
            value: AgentDeclaration::new(harness, name),
        }
    }
    #[doc = concat!(
        "Set the optional nullable `", "mode", "` request field to a value."
    )]
    #[must_use]
    pub fn mode(mut self, mode: String) -> Self {
        self.value.mode = Some(Some(mode));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "mode", "` request field to JSON null."
    )]
    #[must_use]
    pub fn mode_null(mut self) -> Self {
        self.value.mode = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "mode", "` request field.")]
    #[must_use]
    pub fn mode_absent(mut self) -> Self {
        self.value.mode = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "model", "` request field to a value."
    )]
    #[must_use]
    pub fn model(mut self, model: String) -> Self {
        self.value.model = Some(Some(model));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "model", "` request field to JSON null."
    )]
    #[must_use]
    pub fn model_null(mut self) -> Self {
        self.value.model = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "model", "` request field.")]
    #[must_use]
    pub fn model_absent(mut self) -> Self {
        self.value.model = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "thought_level", "` request field to a value."
    )]
    #[must_use]
    pub fn thought_level(mut self, thought_level: String) -> Self {
        self.value.thought_level = Some(Some(thought_level));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "thought_level", "` request field to JSON null."
    )]
    #[must_use]
    pub fn thought_level_null(mut self) -> Self {
        self.value.thought_level = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "thought_level", "` request field.")]
    #[must_use]
    pub fn thought_level_absent(mut self) -> Self {
        self.value.thought_level = None;
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> AgentDeclaration {
        self.value
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionFailedDiagnostic {
    pub context: ConnectionFailedContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionFailedContext {
    pub operation: String,
    pub url: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Completion {
    pub finished_at: String,
    pub started_at: String,
    pub turn_outcome: Option<TurnOutcome>,
}
#[derive(Debug, Clone)]
pub enum TurnOutcome {
    TurnOutcomeAnswered(TurnOutcomeAnswered),
    TurnOutcomeCancelled(TurnOutcomeCancelled),
    TurnOutcomeFailed(TurnOutcomeFailed),
}
impl serde::Serialize for TurnOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::TurnOutcomeAnswered(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(TurnOutcomeAnswered),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("status") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "answered") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "status",
                            stringify!(TurnOutcomeAnswered),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "status",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "status".to_string(),
                            serde_json::Value::String("answered".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::TurnOutcomeCancelled(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(TurnOutcomeCancelled),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("status") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "cancelled") => {
                    }
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "status",
                            stringify!(TurnOutcomeCancelled),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "status",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "status".to_string(),
                            serde_json::Value::String("cancelled".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
            Self::TurnOutcomeFailed(payload) => {
                let mut value = serde_json::to_value(payload).map_err(serde::ser::Error::custom)?;
                let object = value.as_object_mut().ok_or_else(|| {
                    serde::ser::Error::custom(concat!(
                        "discriminated union variant `",
                        stringify!(TurnOutcomeFailed),
                        "` did not serialize as an object",
                    ))
                })?;
                match object.get("status") {
                    Some(serde_json::Value::String(tag)) if matches!(tag.as_str(), "failed") => {}
                    Some(serde_json::Value::String(tag)) => {
                        return Err(serde::ser::Error::custom(format!(
                            "discriminator `{}` value `{tag}` is not valid for variant `{}`",
                            "status",
                            stringify!(TurnOutcomeFailed),
                        )));
                    }
                    Some(_) => {
                        return Err(serde::ser::Error::custom(concat!(
                            "discriminator `",
                            "status",
                            "` did not serialize as a string",
                        )));
                    }
                    None => {
                        object.insert(
                            "status".to_string(),
                            serde_json::Value::String("failed".to_string()),
                        );
                    }
                }
                value.serialize(serializer)
            }
        }
    }
}
impl<'de> serde::Deserialize<'de> for TurnOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let discriminator = match value.get("status") {
            Some(serde_json::Value::String(discriminator)) => Some(discriminator.as_str()),
            Some(_) => {
                return Err(serde::de::Error::custom(concat!(
                    "non-string discriminator `",
                    "status",
                    "`",
                )));
            }
            None => None,
        };
        match discriminator {
            Some(discriminator) => match discriminator {
                "answered" => {
                    let primary_error =
                        match serde_json::from_value::<TurnOutcomeAnswered>(value.clone()) {
                            Ok(payload) => return Ok(Self::TurnOutcomeAnswered(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) =
                        serde_json::from_value::<TurnOutcomeCancelled>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "status",
                                "answered",
                                first_name,
                                stringify!(TurnOutcomeCancelled),
                            )));
                        }
                        structural_match = Some((
                            Self::TurnOutcomeCancelled(payload),
                            stringify!(TurnOutcomeCancelled),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnOutcomeFailed>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "status",
                                "answered",
                                first_name,
                                stringify!(TurnOutcomeFailed),
                            )));
                        }
                        structural_match = Some((
                            Self::TurnOutcomeFailed(payload),
                            stringify!(TurnOutcomeFailed),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "cancelled" => {
                    let primary_error =
                        match serde_json::from_value::<TurnOutcomeCancelled>(value.clone()) {
                            Ok(payload) => return Ok(Self::TurnOutcomeCancelled(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) =
                        serde_json::from_value::<TurnOutcomeAnswered>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "status",
                                "cancelled",
                                first_name,
                                stringify!(TurnOutcomeAnswered),
                            )));
                        }
                        structural_match = Some((
                            Self::TurnOutcomeAnswered(payload),
                            stringify!(TurnOutcomeAnswered),
                        ));
                    }
                    if let Ok(payload) = serde_json::from_value::<TurnOutcomeFailed>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "status",
                                "cancelled",
                                first_name,
                                stringify!(TurnOutcomeFailed),
                            )));
                        }
                        structural_match = Some((
                            Self::TurnOutcomeFailed(payload),
                            stringify!(TurnOutcomeFailed),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                "failed" => {
                    let primary_error =
                        match serde_json::from_value::<TurnOutcomeFailed>(value.clone()) {
                            Ok(payload) => return Ok(Self::TurnOutcomeFailed(payload)),
                            Err(error) => error,
                        };
                    let mut structural_match: Option<(Self, &'static str)> = None;
                    if let Ok(payload) =
                        serde_json::from_value::<TurnOutcomeAnswered>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "status",
                                "failed",
                                first_name,
                                stringify!(TurnOutcomeAnswered),
                            )));
                        }
                        structural_match = Some((
                            Self::TurnOutcomeAnswered(payload),
                            stringify!(TurnOutcomeAnswered),
                        ));
                    }
                    if let Ok(payload) =
                        serde_json::from_value::<TurnOutcomeCancelled>(value.clone())
                    {
                        if let Some((_, first_name)) = &structural_match {
                            return Err(serde::de::Error::custom(format!(
                                "discriminator `{}` value `{}` did not fit its mapped branch and structurally matched both `{}` and `{}`",
                                "status",
                                "failed",
                                first_name,
                                stringify!(TurnOutcomeCancelled),
                            )));
                        }
                        structural_match = Some((
                            Self::TurnOutcomeCancelled(payload),
                            stringify!(TurnOutcomeCancelled),
                        ));
                    }
                    match structural_match {
                        Some((payload, _)) => Ok(payload),
                        None => Err(serde::de::Error::custom(primary_error)),
                    }
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown discriminator value `{other}` for `{}`",
                    "status",
                ))),
            },
            None => Err(serde::de::Error::custom(concat!(
                "missing string discriminator `",
                "status",
                "`",
            ))),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TurnOutcomeFailed {
    pub because: String,
    pub status: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TurnOutcomeCancelled {
    pub status: serde_json::Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TurnOutcomeAnswered {
    pub status: serde_json::Value,
    pub stop_reason: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientFailureDiagnostic {
    pub context: ClientFailureContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientFailureContext {
    pub evidence: Option<String>,
    pub operation: String,
}
///SSE names: open (ChangesOpen) on connect and on reconnect, change (Change) for one resource that changed, and resync (ChangesResync) for a subscriber that fell behind the bounded buffer. No event carries an id.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ChangesEvent {
    Change(Change),
    ChangesOpen(ChangesOpen),
    ChangesResync(ChangesResync),
}
pub type ChangesResync = Refetch;
pub type ChangesOpen = Refetch;
///The event that means refetch every view the Client subscribes to. It carries no fields.
pub type Refetch = serde_json::Value;
///One resource that changed: a Workspace or Session by id, with the name of the Workspace it belongs to, or the Organization's queue, which carries neither. Never a copy of its state.
#[derive(Debug, Clone)]
pub enum Change {
    ChangeWorkspace(ChangeWorkspace),
    ChangeSession(ChangeSession),
    ChangeQueue(ChangeQueue),
}
impl Serialize for Change {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::ChangeWorkspace(value) => serde::Serialize::serialize(value, serializer),
            Self::ChangeSession(value) => serde::Serialize::serialize(value, serializer),
            Self::ChangeQueue(value) => serde::Serialize::serialize(value, serializer),
        }
    }
}
impl<'de> Deserialize<'de> for Change {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn exact_json_integer(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
        }
        fn json_numbers_have_same_value(
            encoded: &serde_json::Number,
            input: &serde_json::Number,
        ) -> bool {
            match (exact_json_integer(encoded), exact_json_integer(input)) {
                (Some(encoded), Some(input)) => encoded == input,
                (Some(encoded), None) => input.as_f64().is_some_and(|input| {
                    input.is_finite() && input.fract() == 0.0 && input as i128 == encoded
                }),
                (None, Some(input)) => encoded.as_f64().is_some_and(|encoded| {
                    encoded.is_finite() && encoded.fract() == 0.0 && encoded as i128 == input
                }),
                (None, None) => encoded.as_f64() == input.as_f64(),
            }
        }
        /// `nulls_may_be_absent` also accepts an input `null` that the
        /// branch omits, as a skipped `None` does. Extra encoded
        /// keys are allowed only by the pre-existing anyOf match.
        fn preserves_complete_json_input(
            encoded: &serde_json::Value,
            input: &serde_json::Value,
            nulls_may_be_absent: bool,
            encoded_keys_may_be_extra: bool,
        ) -> bool {
            match (encoded, input) {
                (serde_json::Value::Object(encoded), serde_json::Value::Object(input)) => {
                    (encoded_keys_may_be_extra
                        || encoded.iter().all(|(key, value)| {
                            input.contains_key(key) || (nulls_may_be_absent && value.is_null())
                        }))
                        && input.iter().all(|(key, value)| match encoded.get(key) {
                            Some(encoded_value) => preserves_complete_json_input(
                                encoded_value,
                                value,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            ),
                            None => nulls_may_be_absent && value.is_null(),
                        })
                }
                (serde_json::Value::Array(encoded), serde_json::Value::Array(input)) => {
                    encoded.len() == input.len()
                        && encoded.iter().zip(input).all(|(encoded, input)| {
                            preserves_complete_json_input(
                                encoded,
                                input,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            )
                        })
                }
                (serde_json::Value::Number(encoded), serde_json::Value::Number(input)) => {
                    json_numbers_have_same_value(encoded, input)
                }
                _ => encoded == input,
            }
        }
        let input = <serde_json::Value as Deserialize>::deserialize(deserializer)?;
        let mut matched = None;
        let mut equivalent = None;
        let mut equivalent_matches = 0usize;
        if input.as_object().is_some_and(|object| {
            true && object.get("resource").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"workspace\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<ChangeWorkspace>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Change),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ChangeWorkspace(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ChangeWorkspace(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("resource").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"session\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<ChangeSession>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Change),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ChangeSession(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ChangeSession(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("resource").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"queue\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<ChangeQueue>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Change),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ChangeQueue(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ChangeQueue(candidate));
                    }
                    _ => {}
                }
            }
        }
        if let Some(matched) = matched {
            return Ok(matched);
        }
        if equivalent_matches > 1 {
            return Err(serde::de::Error::custom(concat!(
                "ambiguous oneOf value for ",
                stringify!(Change),
                ": more than one branch preserved an equivalent input",
            )));
        }
        equivalent.ok_or_else(|| {
            serde::de::Error::custom(concat!(
                "no oneOf branch for ",
                stringify!(Change),
                " preserved the complete input",
            ))
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChangeWorkspace {
    pub id: uuid::Uuid,
    pub resource: serde_json::Value,
    ///The name of the Workspace.
    pub workspace: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChangeSession {
    pub id: uuid::Uuid,
    pub resource: serde_json::Value,
    ///The name of the Workspace that holds the Session.
    pub workspace: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChangeQueue {
    pub resource: serde_json::Value,
}
///The instruction a Workspace started with, rendered by the Trigger that fired or supplied by an operator.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Brief {
    pub brief: BriefContent,
    ///Fields containing payload references; absent means every content field is inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_fields: Option<Vec<String>>,
    pub source: BriefSource,
    pub r#type: serde_json::Value,
}
///Who supplied a Brief: the Trigger whose firing rendered it, or an operator, under the participant name they declared if they declared one.
#[derive(Debug, Clone)]
pub enum BriefSource {
    BriefSourceTrigger(BriefSourceTrigger),
    BriefSourceOperator(BriefSourceOperator),
}
impl Serialize for BriefSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::BriefSourceTrigger(value) => serde::Serialize::serialize(value, serializer),
            Self::BriefSourceOperator(value) => serde::Serialize::serialize(value, serializer),
        }
    }
}
impl<'de> Deserialize<'de> for BriefSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn exact_json_integer(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
        }
        fn json_numbers_have_same_value(
            encoded: &serde_json::Number,
            input: &serde_json::Number,
        ) -> bool {
            match (exact_json_integer(encoded), exact_json_integer(input)) {
                (Some(encoded), Some(input)) => encoded == input,
                (Some(encoded), None) => input.as_f64().is_some_and(|input| {
                    input.is_finite() && input.fract() == 0.0 && input as i128 == encoded
                }),
                (None, Some(input)) => encoded.as_f64().is_some_and(|encoded| {
                    encoded.is_finite() && encoded.fract() == 0.0 && encoded as i128 == input
                }),
                (None, None) => encoded.as_f64() == input.as_f64(),
            }
        }
        /// `nulls_may_be_absent` also accepts an input `null` that the
        /// branch omits, as a skipped `None` does. Extra encoded
        /// keys are allowed only by the pre-existing anyOf match.
        fn preserves_complete_json_input(
            encoded: &serde_json::Value,
            input: &serde_json::Value,
            nulls_may_be_absent: bool,
            encoded_keys_may_be_extra: bool,
        ) -> bool {
            match (encoded, input) {
                (serde_json::Value::Object(encoded), serde_json::Value::Object(input)) => {
                    (encoded_keys_may_be_extra
                        || encoded.iter().all(|(key, value)| {
                            input.contains_key(key) || (nulls_may_be_absent && value.is_null())
                        }))
                        && input.iter().all(|(key, value)| match encoded.get(key) {
                            Some(encoded_value) => preserves_complete_json_input(
                                encoded_value,
                                value,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            ),
                            None => nulls_may_be_absent && value.is_null(),
                        })
                }
                (serde_json::Value::Array(encoded), serde_json::Value::Array(input)) => {
                    encoded.len() == input.len()
                        && encoded.iter().zip(input).all(|(encoded, input)| {
                            preserves_complete_json_input(
                                encoded,
                                input,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            )
                        })
                }
                (serde_json::Value::Number(encoded), serde_json::Value::Number(input)) => {
                    json_numbers_have_same_value(encoded, input)
                }
                _ => encoded == input,
            }
        }
        let input = <serde_json::Value as Deserialize>::deserialize(deserializer)?;
        let mut matched = None;
        let mut equivalent = None;
        let mut equivalent_matches = 0usize;
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"trigger\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<BriefSourceTrigger>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(BriefSource),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::BriefSourceTrigger(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::BriefSourceTrigger(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("kind").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"operator\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<BriefSourceOperator>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(BriefSource),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::BriefSourceOperator(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::BriefSourceOperator(candidate));
                    }
                    _ => {}
                }
            }
        }
        if let Some(matched) = matched {
            return Ok(matched);
        }
        if equivalent_matches > 1 {
            return Err(serde::de::Error::custom(concat!(
                "ambiguous oneOf value for ",
                stringify!(BriefSource),
                ": more than one branch preserved an equivalent input",
            )));
        }
        equivalent.ok_or_else(|| {
            serde::de::Error::custom(concat!(
                "no oneOf branch for ",
                stringify!(BriefSource),
                " preserved the complete input",
            ))
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BriefSourceTrigger {
    pub kind: serde_json::Value,
    pub trigger: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BriefSourceOperator {
    pub kind: serde_json::Value,
    ///The participant who joined directly before the Brief, or null when the operator declared no name and no one joined.
    pub participant: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BriefContent {
    InlineBrief(InlineBrief),
    PayloadReference(PayloadReference),
}
///A body field exceeding 65536 bytes, fetched under its owning Workspace; message, thought and brief text use raw UTF-8, while plans, batched messages and tool input/results use compact JSON.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadReference {
    ///Constraint: minimum=65537
    pub bytes: i64,
    pub media_type: PayloadReferenceMediaType,
    ///Constraint: pattern=`^[0-9a-f-]+:[1-9][0-9]*:[a-z_]+$`
    pub payload_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum PayloadReferenceMediaType {
    #[default]
    #[serde(rename = "text/plain; charset=utf-8")]
    TextPlainCharsetUtf8,
    #[serde(rename = "application/json")]
    ApplicationJson,
}
impl PayloadReferenceMediaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TextPlainCharsetUtf8 => "text/plain; charset=utf-8",
            Self::ApplicationJson => "application/json",
        }
    }
}
impl ::std::fmt::Display for PayloadReferenceMediaType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for PayloadReferenceMediaType {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
pub type InlineBrief = String;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthenticationFailedDiagnostic {
    pub context: AuthenticationFailedContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthenticationFailedContext {
    pub covered: Option<bool>,
    pub evidence: AuthenticationRequiredEvidence,
    ///Established only from evidence; never inferred from arbitrary harness output.
    pub expired: Option<bool>,
    pub harness: String,
    pub image: Option<String>,
    pub session: String,
    pub sign_in: Option<String>,
}
///The agent answered with ACP's authentication-required error.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthenticationRequiredEvidence {
    pub code: i64,
    pub kind: serde_json::Value,
    ///The ACP auth method the supervisor was configured to log in with.
    pub method: Option<String>,
    ///The ids of the ACP auth methods the agent offered.
    pub methods: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppliedTriggers {
    pub admitting_outsiders: Vec<String>,
    pub changes: Vec<AppliedTriggersChangesItem>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppliedTriggersChangesItem {
    pub action: AppliedTriggersChangesItemAction,
    pub differences: Vec<AppliedTriggersChangesItemDifferencesItem>,
    pub name: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppliedTriggersChangesItemDifferencesItem {
    pub becomes: Option<String>,
    pub field: String,
    pub was: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum AppliedTriggersChangesItemAction {
    #[default]
    #[serde(rename = "add")]
    Add,
    #[serde(rename = "change")]
    Change,
    #[serde(rename = "remove")]
    Remove,
}
impl AppliedTriggersChangesItemAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Change => "change",
            Self::Remove => "remove",
        }
    }
}
impl ::std::fmt::Display for AppliedTriggersChangesItemAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for AppliedTriggersChangesItemAction {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppliedDeclaration {
    pub admitting_outsiders: Vec<String>,
    pub declarations: Vec<DeclarationDiff>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclarationDiff {
    pub action: DeclarationDiffAction,
    pub differences: Vec<DeclarationDiffDifferencesItem>,
    pub kind: DeclarationDiffKind,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum DeclarationDiffKind {
    #[default]
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "trigger")]
    Trigger,
}
impl DeclarationDiffKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Agent => "agent",
            Self::Trigger => "trigger",
        }
    }
}
impl ::std::fmt::Display for DeclarationDiffKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for DeclarationDiffKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclarationDiffDifferencesItem {
    pub becomes: Option<String>,
    pub field: String,
    pub was: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum DeclarationDiffAction {
    #[default]
    #[serde(rename = "add")]
    Add,
    #[serde(rename = "change")]
    Change,
    #[serde(rename = "unchanged")]
    Unchanged,
}
impl DeclarationDiffAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Change => "change",
            Self::Unchanged => "unchanged",
        }
    }
}
impl ::std::fmt::Display for DeclarationDiffAction {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for DeclarationDiffAction {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AmbiguousReferenceDiagnostic {
    pub context: AmbiguousReferenceContext,
    pub field: Option<String>,
    pub kind: serde_json::Value,
    pub message: String,
    pub next_steps: Vec<Action>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AmbiguousReferenceContext {
    pub candidates: Vec<Candidate>,
    pub organization: Option<String>,
    pub reference: String,
    pub resource: Resource,
}
///One record an ambiguous reference could have meant.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Candidate {
    pub id: String,
    pub name: String,
}
///Read-time summary of omitted narration and detail between consecutive shared-state entries. first_seq identifies the whole Activity, including selected kinds, and stays stable across pages and reconnects. last_seq is the highest Activity seq examined; its SSE id advances the global cursor. A closed replacement precedes the closing shared-state entry even when no detail was added. No summary is stored.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Activity {
    ///An omitted call was interrupted or unresolved.
    pub anomaly: bool,
    ///The next shared-state boundary has been examined. Followers replace open summaries by first_seq and CLI followers print only this final replacement.
    pub closed: bool,
    pub counts: ActivityCounts,
    ///Latest finish among omitted unexpired entries.
    pub finished_at: Option<String>,
    ///Constraint: minimum=1
    pub first_seq: i64,
    ///Constraint: minimum=1
    pub last_seq: i64,
    pub latest: Option<ActivityLatest>,
    ///Earliest start among omitted unexpired entries.
    pub started_at: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActivityLatest {
    pub kind: TranscriptKind,
    pub status: Option<String>,
    pub title: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum TranscriptKind {
    #[default]
    #[serde(rename = "shared_state")]
    SharedState,
    #[serde(rename = "narration")]
    Narration,
    #[serde(rename = "detail")]
    Detail,
}
impl TranscriptKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SharedState => "shared_state",
            Self::Narration => "narration",
            Self::Detail => "detail",
        }
    }
}
impl ::std::fmt::Display for TranscriptKind {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for TranscriptKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActivityCounts {
    ///Constraint: minimum=0
    pub failed_calls: i64,
    ///Constraint: minimum=0
    pub plans: i64,
    ///Constraint: minimum=0
    pub thoughts: i64,
    ///Constraint: minimum=0
    pub tombstones: i64,
    ///Constraint: minimum=0
    pub tool_calls: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActiveWork {
    ///Occupants of other Organizations, counted and never named.
    ///Constraint: minimum=0
    pub elsewhere: i64,
    ///The recorded Active-Work Slot limit, or null when dispatch configuration is unknown.
    ///Constraint: minimum=1
    pub limit: Option<i64>,
    ///This Organization's working and trailing Sessions, in the order they were enqueued.
    pub occupants: Vec<Occupant>,
    ///Every working or trailing Session on the control plane: the pool it counts against is shared by every Organization.
    ///Constraint: minimum=0
    pub occupied: i64,
}
///A Session holding an Active-Work Slot. A trailing one answered its Turn while work its agent started still runs; it is never among the Waiting Sessions.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Occupant {
    pub agent: String,
    pub enqueued_at: String,
    pub name: String,
    pub phase: OccupantPhase,
    pub workspace: uuid::Uuid,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum OccupantPhase {
    #[default]
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "trailing")]
    Trailing,
}
impl OccupantPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Trailing => "trailing",
        }
    }
}
impl ::std::fmt::Display for OccupantPhase {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for OccupantPhase {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
///A typed corrective, inspective or retry step a Diagnostic offers, naming its own inputs rather than a shell command or browser route (ADR-0052). A Client binds it to a form, a flag or an API call.
#[derive(Debug, Clone)]
pub enum Action {
    NameOperatorAction(NameOperatorAction),
    DeclareOrganizationAction(DeclareOrganizationAction),
    DeclareProjectAction(DeclareProjectAction),
    DeclareAgentAction(DeclareAgentAction),
    DeclareSubscriptionProfileAction(DeclareSubscriptionProfileAction),
    SetProviderCredentialAction(SetProviderCredentialAction),
    InspectResourceAction(InspectResourceAction),
    ListResourcesAction(ListResourcesAction),
    StopSessionAction(StopSessionAction),
    EnqueueSessionAction(EnqueueSessionAction),
    EnableIntegrationAction(EnableIntegrationAction),
    ReleaseInstanceAction(ReleaseInstanceAction),
    CorrectFieldAction(CorrectFieldAction),
    SignInAction(SignInAction),
    InspectHarnessImageAction(InspectHarnessImageAction),
    CheckConnectionAction(CheckConnectionAction),
    RetryReadAction(RetryReadAction),
    InspectOperationAction(InspectOperationAction),
}
impl Serialize for Action {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::NameOperatorAction(value) => serde::Serialize::serialize(value, serializer),
            Self::DeclareOrganizationAction(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::DeclareProjectAction(value) => serde::Serialize::serialize(value, serializer),
            Self::DeclareAgentAction(value) => serde::Serialize::serialize(value, serializer),
            Self::DeclareSubscriptionProfileAction(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::SetProviderCredentialAction(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::InspectResourceAction(value) => serde::Serialize::serialize(value, serializer),
            Self::ListResourcesAction(value) => serde::Serialize::serialize(value, serializer),
            Self::StopSessionAction(value) => serde::Serialize::serialize(value, serializer),
            Self::EnqueueSessionAction(value) => serde::Serialize::serialize(value, serializer),
            Self::EnableIntegrationAction(value) => serde::Serialize::serialize(value, serializer),
            Self::ReleaseInstanceAction(value) => serde::Serialize::serialize(value, serializer),
            Self::CorrectFieldAction(value) => serde::Serialize::serialize(value, serializer),
            Self::SignInAction(value) => serde::Serialize::serialize(value, serializer),
            Self::InspectHarnessImageAction(value) => {
                serde::Serialize::serialize(value, serializer)
            }
            Self::CheckConnectionAction(value) => serde::Serialize::serialize(value, serializer),
            Self::RetryReadAction(value) => serde::Serialize::serialize(value, serializer),
            Self::InspectOperationAction(value) => serde::Serialize::serialize(value, serializer),
        }
    }
}
impl<'de> Deserialize<'de> for Action {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        fn exact_json_integer(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
        }
        fn json_numbers_have_same_value(
            encoded: &serde_json::Number,
            input: &serde_json::Number,
        ) -> bool {
            match (exact_json_integer(encoded), exact_json_integer(input)) {
                (Some(encoded), Some(input)) => encoded == input,
                (Some(encoded), None) => input.as_f64().is_some_and(|input| {
                    input.is_finite() && input.fract() == 0.0 && input as i128 == encoded
                }),
                (None, Some(input)) => encoded.as_f64().is_some_and(|encoded| {
                    encoded.is_finite() && encoded.fract() == 0.0 && encoded as i128 == input
                }),
                (None, None) => encoded.as_f64() == input.as_f64(),
            }
        }
        /// `nulls_may_be_absent` also accepts an input `null` that the
        /// branch omits, as a skipped `None` does. Extra encoded
        /// keys are allowed only by the pre-existing anyOf match.
        fn preserves_complete_json_input(
            encoded: &serde_json::Value,
            input: &serde_json::Value,
            nulls_may_be_absent: bool,
            encoded_keys_may_be_extra: bool,
        ) -> bool {
            match (encoded, input) {
                (serde_json::Value::Object(encoded), serde_json::Value::Object(input)) => {
                    (encoded_keys_may_be_extra
                        || encoded.iter().all(|(key, value)| {
                            input.contains_key(key) || (nulls_may_be_absent && value.is_null())
                        }))
                        && input.iter().all(|(key, value)| match encoded.get(key) {
                            Some(encoded_value) => preserves_complete_json_input(
                                encoded_value,
                                value,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            ),
                            None => nulls_may_be_absent && value.is_null(),
                        })
                }
                (serde_json::Value::Array(encoded), serde_json::Value::Array(input)) => {
                    encoded.len() == input.len()
                        && encoded.iter().zip(input).all(|(encoded, input)| {
                            preserves_complete_json_input(
                                encoded,
                                input,
                                nulls_may_be_absent,
                                encoded_keys_may_be_extra,
                            )
                        })
                }
                (serde_json::Value::Number(encoded), serde_json::Value::Number(input)) => {
                    json_numbers_have_same_value(encoded, input)
                }
                _ => encoded == input,
            }
        }
        let input = <serde_json::Value as Deserialize>::deserialize(deserializer)?;
        let mut matched = None;
        let mut equivalent = None;
        let mut equivalent_matches = 0usize;
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"name_operator\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<NameOperatorAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::NameOperatorAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::NameOperatorAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"declare_organization\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<DeclareOrganizationAction>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::DeclareOrganizationAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::DeclareOrganizationAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"declare_project\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<DeclareProjectAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::DeclareProjectAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::DeclareProjectAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"declare_agent\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<DeclareAgentAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::DeclareAgentAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::DeclareAgentAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null()
                    || matches!(
                        value.to_string().as_str(),
                        "\"declare_subscription_profile\""
                    )
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<DeclareSubscriptionProfileAction>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::DeclareSubscriptionProfileAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::DeclareSubscriptionProfileAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null()
                    || matches!(value.to_string().as_str(), "\"set_provider_credential\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<SetProviderCredentialAction>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::SetProviderCredentialAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::SetProviderCredentialAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"inspect_resource\"")
            }) && object.get("resource").is_some_and(|value| {
                value.is_null()
                    || matches!(
                        value.to_string().as_str(),
                        "\"organization\""
                            | "\"project\""
                            | "\"agent\""
                            | "\"subscription_profile\""
                            | "\"provider_credential\""
                            | "\"integration\""
                            | "\"trigger\""
                            | "\"event\""
                            | "\"workspace\""
                            | "\"session\""
                            | "\"instance\""
                            | "\"held_message\""
                            | "\"transcript_payload\""
                    )
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<InspectResourceAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::InspectResourceAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::InspectResourceAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"list_resources\"")
            }) && object.get("resource").is_some_and(|value| {
                value.is_null()
                    || matches!(
                        value.to_string().as_str(),
                        "\"organization\""
                            | "\"project\""
                            | "\"agent\""
                            | "\"subscription_profile\""
                            | "\"provider_credential\""
                            | "\"integration\""
                            | "\"trigger\""
                            | "\"event\""
                            | "\"workspace\""
                            | "\"session\""
                            | "\"instance\""
                            | "\"held_message\""
                            | "\"transcript_payload\""
                    )
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<ListResourcesAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ListResourcesAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ListResourcesAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"stop_session\"")
            }) && object.get("effect").is_some_and(|value| {
                value.is_null()
                    || matches!(
                        value.to_string().as_str(),
                        "\"fails_session\"" | "\"ends_session\"" | "\"discards_unpublished_work\""
                    )
            }) && object.get("requires_choice").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "true")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<StopSessionAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::StopSessionAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::StopSessionAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"enqueue_session\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<EnqueueSessionAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::EnqueueSessionAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::EnqueueSessionAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"enable_integration\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<EnableIntegrationAction>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::EnableIntegrationAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::EnableIntegrationAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"release_instance\"")
            }) && object.get("effect").is_some_and(|value| {
                value.is_null()
                    || matches!(
                        value.to_string().as_str(),
                        "\"fails_session\"" | "\"ends_session\"" | "\"discards_unpublished_work\""
                    )
            }) && object.get("requires_choice").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "true")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<ReleaseInstanceAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::ReleaseInstanceAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::ReleaseInstanceAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"correct_field\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<CorrectFieldAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::CorrectFieldAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::CorrectFieldAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"sign_in\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<SignInAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::SignInAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::SignInAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"inspect_harness_image\"")
            })
        }) {
            if let Ok(candidate) =
                serde_json::from_value::<InspectHarnessImageAction>(input.clone())
            {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::InspectHarnessImageAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::InspectHarnessImageAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"check_connection\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<CheckConnectionAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::CheckConnectionAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::CheckConnectionAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"retry_read\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<RetryReadAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::RetryReadAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::RetryReadAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if input.as_object().is_some_and(|object| {
            true && object.get("action").is_some_and(|value| {
                value.is_null() || matches!(value.to_string().as_str(), "\"inspect_operation\"")
            })
        }) {
            if let Ok(candidate) = serde_json::from_value::<InspectOperationAction>(input.clone()) {
                match serde_json::to_value(&candidate) {
                    Ok(encoded) if encoded == input => {
                        if matched.is_some() {
                            return Err(serde::de::Error::custom(concat!(
                                "ambiguous oneOf value for ",
                                stringify!(Action),
                                ": more than one branch preserved the complete input",
                            )));
                        }
                        matched = Some(Self::InspectOperationAction(candidate));
                    }
                    Ok(encoded) if preserves_complete_json_input(&encoded, &input, true, false) => {
                        equivalent_matches += 1;
                        equivalent.get_or_insert(Self::InspectOperationAction(candidate));
                    }
                    _ => {}
                }
            }
        }
        if let Some(matched) = matched {
            return Ok(matched);
        }
        if equivalent_matches > 1 {
            return Err(serde::de::Error::custom(concat!(
                "ambiguous oneOf value for ",
                stringify!(Action),
                ": more than one branch preserved an equivalent input",
            )));
        }
        equivalent.ok_or_else(|| {
            serde::de::Error::custom(concat!(
                "no oneOf branch for ",
                stringify!(Action),
                " preserved the complete input",
            ))
        })
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StopSessionAction {
    pub action: serde_json::Value,
    ///Display-only explanation of the destructive effect; wording never decides whether confirmation is required.
    pub consequence: String,
    pub effect: Consequence,
    pub organization: String,
    pub requires_choice: ExplicitChoice,
    pub session: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignInAction {
    pub action: serde_json::Value,
    pub harness: Option<String>,
    pub method: Option<String>,
    ///A safe locator for which sign-in to use, never held material.
    pub sign_in: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetProviderCredentialAction {
    pub action: serde_json::Value,
    ///The credential's variable. Its value is always collected privately and never serialized here.
    pub name: String,
    pub organization: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryReadAction {
    pub action: serde_json::Value,
    pub operation: String,
    pub resource: Option<String>,
    pub retry_after_seconds: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleaseInstanceAction {
    pub action: serde_json::Value,
    ///Display-only explanation of the destructive effect; wording never decides whether confirmation is required.
    pub consequence: String,
    pub effect: Consequence,
    pub instance: Option<String>,
    pub organization: String,
    pub requires_choice: ExplicitChoice,
    pub workspace: String,
}
///A destructive step is taken only by an explicit choice, whatever its consequence says; a Client never runs it as a default or automatic repair.
pub type ExplicitChoice = serde_json::Value;
///What a destructive step does to work, typed so a Client never reads it from the consequence sentence.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum Consequence {
    #[default]
    #[serde(rename = "fails_session")]
    FailsSession,
    #[serde(rename = "ends_session")]
    EndsSession,
    #[serde(rename = "discards_unpublished_work")]
    DiscardsUnpublishedWork,
}
impl Consequence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FailsSession => "fails_session",
            Self::EndsSession => "ends_session",
            Self::DiscardsUnpublishedWork => "discards_unpublished_work",
        }
    }
}
impl ::std::fmt::Display for Consequence {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for Consequence {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NameOperatorAction {
    pub action: serde_json::Value,
    pub missing: Vec<String>,
    pub name: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListResourcesAction {
    pub action: serde_json::Value,
    pub organization: Option<String>,
    pub resource: Resource,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InspectResourceAction {
    pub action: serde_json::Value,
    pub organization: Option<String>,
    pub reference: Option<String>,
    pub resource: Resource,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InspectOperationAction {
    pub action: serde_json::Value,
    pub operation: String,
    pub resource: Option<String>,
    ///Whether the operation's result is unknown, used after a lost write response. Never a signal to replay the write.
    pub uncertain: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InspectHarnessImageAction {
    pub action: serde_json::Value,
    pub command: Option<String>,
    pub harness: Option<String>,
    pub image: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnqueueSessionAction {
    pub action: serde_json::Value,
    pub missing: Vec<String>,
    pub organization: String,
    pub workspace: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnableIntegrationAction {
    pub action: serde_json::Value,
    pub integration: String,
    pub organization: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclareSubscriptionProfileAction {
    pub action: serde_json::Value,
    ///Which of this action's fixed inputs are not yet known, from `name`, `owner`.
    pub missing: Vec<String>,
    pub name: Option<String>,
    pub organization: String,
    pub owner: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclareProjectAction {
    pub action: serde_json::Value,
    pub branch: Option<String>,
    ///Which of this action's fixed inputs are not yet known, from `name`, `repositories`, `branch`.
    pub missing: Vec<String>,
    pub name: Option<String>,
    pub organization: String,
    pub repositories: Option<Vec<String>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclareOrganizationAction {
    pub action: serde_json::Value,
    ///Which of this action's fixed inputs are not yet known, from `name`.
    pub missing: Vec<String>,
    pub name: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeclareAgentAction {
    pub action: serde_json::Value,
    pub harness: Option<String>,
    ///Which of this action's fixed inputs are not yet known, from `name`, `harness`.
    pub missing: Vec<String>,
    pub name: Option<String>,
    pub organization: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorrectFieldAction {
    pub action: serde_json::Value,
    pub allowed_values: Option<Vec<String>>,
    pub constraint: String,
    pub field: String,
    pub operation: String,
    pub resource: Option<Resource>,
}
///A kind of record a reference, a declaration or an action names (ADR-0052).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum Resource {
    #[default]
    #[serde(rename = "organization")]
    Organization,
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "subscription_profile")]
    SubscriptionProfile,
    #[serde(rename = "provider_credential")]
    ProviderCredential,
    #[serde(rename = "integration")]
    Integration,
    #[serde(rename = "trigger")]
    Trigger,
    #[serde(rename = "event")]
    Event,
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "instance")]
    Instance,
    #[serde(rename = "held_message")]
    HeldMessage,
    #[serde(rename = "transcript_payload")]
    TranscriptPayload,
}
impl Resource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::Project => "project",
            Self::Agent => "agent",
            Self::SubscriptionProfile => "subscription_profile",
            Self::ProviderCredential => "provider_credential",
            Self::Integration => "integration",
            Self::Trigger => "trigger",
            Self::Event => "event",
            Self::Workspace => "workspace",
            Self::Session => "session",
            Self::Instance => "instance",
            Self::HeldMessage => "held_message",
            Self::TranscriptPayload => "transcript_payload",
        }
    }
}
impl ::std::fmt::Display for Resource {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl AsRef<str> for Resource {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CheckConnectionAction {
    pub action: serde_json::Value,
    ///Whether Compose-specific checks apply, known only from the selected deployment's own evidence.
    pub compose: bool,
    pub service: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentModel {
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub model: Option<Option<String>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileText {
    pub path: String,
    pub text: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct InstanceRelease {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participant: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Operator {
    pub id: uuid::Uuid,
    pub name: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OperatorDeclaration {
    ///Constraint: minLength=1
    pub name: String,
}
///One option a person changes on a Session: exactly one of `option` (the id the harness reported) or `category` (model, mode or thought_level), and the value to set.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OptionChange {
    ///One of model, mode or thought_level. Exactly one of option or category.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub category: Option<Option<String>>,
    ///The harness option's id. Exactly one of option or category.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub option: Option<Option<String>>,
    ///The person making the change, by the participant name rule.
    ///Constraint: minLength=1
    pub participant: String,
    ///Constraint: minLength=1
    pub value: String,
}
impl OptionChange {
    /// Construct this request with every required wire field.
    pub fn new(participant: String, value: String) -> Self {
        Self {
            participant,
            value,
            category: None,
            option: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(participant: String, value: String) -> OptionChangeBuilder {
        OptionChangeBuilder::new(participant, value)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct OptionChangeBuilder {
    value: OptionChange,
}
impl OptionChangeBuilder {
    /// Start a builder with every required wire field.
    pub fn new(participant: String, value: String) -> Self {
        Self {
            value: OptionChange::new(participant, value),
        }
    }
    #[doc = concat!(
        "Set the optional nullable `", "category", "` request field to a value."
    )]
    #[must_use]
    pub fn category(mut self, category: String) -> Self {
        self.value.category = Some(Some(category));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "category", "` request field to JSON null."
    )]
    #[must_use]
    pub fn category_null(mut self) -> Self {
        self.value.category = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "category", "` request field.")]
    #[must_use]
    pub fn category_absent(mut self) -> Self {
        self.value.category = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "option", "` request field to a value."
    )]
    #[must_use]
    pub fn option(mut self, option: String) -> Self {
        self.value.option = Some(Some(option));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "option", "` request field to JSON null."
    )]
    #[must_use]
    pub fn option_null(mut self) -> Self {
        self.value.option = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "option", "` request field.")]
    #[must_use]
    pub fn option_absent(mut self) -> Self {
        self.value.option = None;
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> OptionChange {
        self.value
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrganizationDeclaration {
    ///The most active, idle, or held Instances this Organization may keep live. Null means unbounded.
    ///Constraint: minimum=1
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub max_live_instances: Option<Option<i64>>,
    ///Constraint: minLength=1
    pub name: String,
}
impl OrganizationDeclaration {
    /// Construct this request with every required wire field.
    pub fn new(name: String) -> Self {
        Self {
            name,
            max_live_instances: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(name: String) -> OrganizationDeclarationBuilder {
        OrganizationDeclarationBuilder::new(name)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct OrganizationDeclarationBuilder {
    value: OrganizationDeclaration,
}
impl OrganizationDeclarationBuilder {
    /// Start a builder with every required wire field.
    pub fn new(name: String) -> Self {
        Self {
            value: OrganizationDeclaration::new(name),
        }
    }
    #[doc = concat!(
        "Set the optional nullable `", "max_live_instances",
        "` request field to a value."
    )]
    #[must_use]
    pub fn max_live_instances(mut self, max_live_instances: i64) -> Self {
        self.value.max_live_instances = Some(Some(max_live_instances));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "max_live_instances",
        "` request field to JSON null."
    )]
    #[must_use]
    pub fn max_live_instances_null(mut self) -> Self {
        self.value.max_live_instances = Some(None);
        self
    }
    #[doc = concat!(
        "Omit the optional nullable `", "max_live_instances", "` request field."
    )]
    #[must_use]
    pub fn max_live_instances_absent(mut self) -> Self {
        self.value.max_live_instances = None;
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> OrganizationDeclaration {
        self.value
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderCredentialSecret {
    ///Constraint: minLength=1
    pub secret: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleasedInstance {
    pub instance: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SessionDeclaration {
    ///Omitted, the Agent of the Workspace's latest Session.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub agent: Option<Option<String>>,
    ///Sessions of the Organization the new Session waits on, each named as a Session is anywhere else. It is dispatched only once every one has ended successfully. One the Organization does not have is refused with 404, and one that ended without success or is unreachable with 409; either refusal leaves nothing behind. No request adds a Dependency to a Session that already exists.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub depends_on: Option<Option<Vec<String>>>,
    ///Omitted, whatever the Agent names.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub mode: Option<Option<String>>,
    ///Omitted, whatever the Agent names.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub model: Option<Option<String>>,
    ///Omitted, whatever the Agent names.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub thought_level: Option<Option<String>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionInterrupt {
    ///The person asking. Trimmed, 1 to 64 characters with no control characters, and never the name of an Agent in the Organization.
    pub participant: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartGithubAppRequest {
    ///GitHub API origin; defaults to https://api.github.com
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub api: Option<Option<String>>,
    ///GitHub organization that will own the private App; omit for your personal account
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub app_organization: Option<Option<String>>,
    ///Browser-reachable loopback HTTP(S) origin
    pub callback_base: String,
    pub name: String,
    pub repository: String,
    ///Public HTTPS origin of the webhook listener; omit for polling
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub webhook_base: Option<Option<String>>,
}
impl StartGithubAppRequest {
    /// Construct this request with every required wire field.
    pub fn new(callback_base: String, name: String, repository: String) -> Self {
        Self {
            callback_base,
            name,
            repository,
            api: None,
            app_organization: None,
            webhook_base: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(
        callback_base: String,
        name: String,
        repository: String,
    ) -> StartGithubAppRequestBuilder {
        StartGithubAppRequestBuilder::new(callback_base, name, repository)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct StartGithubAppRequestBuilder {
    value: StartGithubAppRequest,
}
impl StartGithubAppRequestBuilder {
    /// Start a builder with every required wire field.
    pub fn new(callback_base: String, name: String, repository: String) -> Self {
        Self {
            value: StartGithubAppRequest::new(callback_base, name, repository),
        }
    }
    #[doc = concat!("Set the optional nullable `", "api", "` request field to a value.")]
    #[must_use]
    pub fn api(mut self, api: String) -> Self {
        self.value.api = Some(Some(api));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "api", "` request field to JSON null."
    )]
    #[must_use]
    pub fn api_null(mut self) -> Self {
        self.value.api = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "api", "` request field.")]
    #[must_use]
    pub fn api_absent(mut self) -> Self {
        self.value.api = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "app_organization", "` request field to a value."
    )]
    #[must_use]
    pub fn app_organization(mut self, app_organization: String) -> Self {
        self.value.app_organization = Some(Some(app_organization));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "app_organization",
        "` request field to JSON null."
    )]
    #[must_use]
    pub fn app_organization_null(mut self) -> Self {
        self.value.app_organization = Some(None);
        self
    }
    #[doc = concat!(
        "Omit the optional nullable `", "app_organization", "` request field."
    )]
    #[must_use]
    pub fn app_organization_absent(mut self) -> Self {
        self.value.app_organization = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "webhook_base", "` request field to a value."
    )]
    #[must_use]
    pub fn webhook_base(mut self, webhook_base: String) -> Self {
        self.value.webhook_base = Some(Some(webhook_base));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "webhook_base", "` request field to JSON null."
    )]
    #[must_use]
    pub fn webhook_base_null(mut self) -> Self {
        self.value.webhook_base = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "webhook_base", "` request field.")]
    #[must_use]
    pub fn webhook_base_absent(mut self) -> Self {
        self.value.webhook_base = None;
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> StartGithubAppRequest {
        self.value
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartGithubAppResponse201 {
    pub url: String,
}
///The event's name and `data` are those of the per-resource stream the subscription follows: an `Event` for a Transcript, a `ChangesEvent` for notices.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamEvent {
    ///The subscription's Transcript cursor after this event, on entry, Activity and cursor events only. It resumes the subscription as `after`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    ///The per-resource stream's payload for this event name.
    pub data: serde_json::Value,
    pub subscription: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamReservation {
    pub token: uuid::Uuid,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscriptionProfileDeclaration {
    ///Constraint: minLength=1
    pub name: String,
    ///The person it belongs to, which never changes.
    ///Constraint: minLength=1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}
impl SubscriptionProfileDeclaration {
    /// Construct this request with every required wire field.
    pub fn new(name: String) -> Self {
        Self { name, owner: None }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(name: String) -> SubscriptionProfileDeclarationBuilder {
        SubscriptionProfileDeclarationBuilder::new(name)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct SubscriptionProfileDeclarationBuilder {
    value: SubscriptionProfileDeclaration,
}
impl SubscriptionProfileDeclarationBuilder {
    /// Start a builder with every required wire field.
    pub fn new(name: String) -> Self {
        Self {
            value: SubscriptionProfileDeclaration::new(name),
        }
    }
    #[doc = concat!("Set the optional `", "owner", "` request field.")]
    #[must_use]
    pub fn owner(mut self, owner: String) -> Self {
        self.value.owner = Some(owner);
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> SubscriptionProfileDeclaration {
        self.value
    }
}
pub type TranscriptPayloadResponse = serde_json::Value;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerDispatch {
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub agent: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub instruction: Option<Option<String>>,
    pub integration: String,
    pub issue: i64,
}
impl TriggerDispatch {
    /// Construct this request with every required wire field.
    pub fn new(integration: String, issue: i64) -> Self {
        Self {
            integration,
            issue,
            agent: None,
            instruction: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(integration: String, issue: i64) -> TriggerDispatchBuilder {
        TriggerDispatchBuilder::new(integration, issue)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct TriggerDispatchBuilder {
    value: TriggerDispatch,
}
impl TriggerDispatchBuilder {
    /// Start a builder with every required wire field.
    pub fn new(integration: String, issue: i64) -> Self {
        Self {
            value: TriggerDispatch::new(integration, issue),
        }
    }
    #[doc = concat!(
        "Set the optional nullable `", "agent", "` request field to a value."
    )]
    #[must_use]
    pub fn agent(mut self, agent: String) -> Self {
        self.value.agent = Some(Some(agent));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "agent", "` request field to JSON null."
    )]
    #[must_use]
    pub fn agent_null(mut self) -> Self {
        self.value.agent = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "agent", "` request field.")]
    #[must_use]
    pub fn agent_absent(mut self) -> Self {
        self.value.agent = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "instruction", "` request field to a value."
    )]
    #[must_use]
    pub fn instruction(mut self, instruction: String) -> Self {
        self.value.instruction = Some(Some(instruction));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "instruction", "` request field to JSON null."
    )]
    #[must_use]
    pub fn instruction_null(mut self) -> Self {
        self.value.instruction = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "instruction", "` request field.")]
    #[must_use]
    pub fn instruction_absent(mut self) -> Self {
        self.value.instruction = None;
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> TriggerDispatch {
        self.value
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceDeclaration {
    ///Constraint: minLength=1
    pub agent: String,
    ///Omitted, a branch of the Workspace's own. A continuation runs on the branch of the Workspace it continues.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub branch: Option<Option<String>>,
    ///The Brief the Workspace starts with, handed to the agent exactly as given. Omitted, the first Session waits for its first message to become the Brief.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub brief: Option<Option<String>>,
    ///The sealed Workspace this one carries on from, by its generated name, its identifier, any unambiguous prefix of its identifier, or `latest`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub continues: Option<Option<String>>,
    ///Sessions of the Organization the new Session waits on, each named as a Session is anywhere else. It is dispatched only once every one has ended successfully. One the Organization does not have is refused with 404, and one that ended without success or is unreachable with 409; either refusal leaves nothing behind. No request adds a Dependency to a Session that already exists.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub depends_on: Option<Option<Vec<String>>>,
    ///The mode the first Session runs in. Omitted, the Agent's, then the Harness's default.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub mode: Option<Option<String>>,
    ///The model the first Session runs on. Omitted, the Agent's, then the Harness's default.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub model: Option<Option<String>>,
    ///The name the Brief is written under. Omitted, its source is the operator and no one joins.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub participant: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub profile: Option<Option<String>>,
    ///Constraint: minLength=1
    pub project: String,
    ///The thought level the first Session runs at. Omitted, the Agent's, then the Harness's default.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "tri_state_serde::deserialize"
    )]
    pub thought_level: Option<Option<String>>,
}
impl WorkspaceDeclaration {
    /// Construct this request with every required wire field.
    pub fn new(agent: String, project: String) -> Self {
        Self {
            agent,
            project,
            branch: None,
            brief: None,
            continues: None,
            depends_on: None,
            mode: None,
            model: None,
            participant: None,
            profile: None,
            thought_level: None,
        }
    }
    /// Start a dependency-free builder with every required wire field.
    pub fn builder(agent: String, project: String) -> WorkspaceDeclarationBuilder {
        WorkspaceDeclarationBuilder::new(agent, project)
    }
}
/// Dependency-free builder for [`#struct_name`].
#[derive(Debug, Clone)]
#[must_use]
pub struct WorkspaceDeclarationBuilder {
    value: WorkspaceDeclaration,
}
impl WorkspaceDeclarationBuilder {
    /// Start a builder with every required wire field.
    pub fn new(agent: String, project: String) -> Self {
        Self {
            value: WorkspaceDeclaration::new(agent, project),
        }
    }
    #[doc = concat!(
        "Set the optional nullable `", "branch", "` request field to a value."
    )]
    #[must_use]
    pub fn branch(mut self, branch: String) -> Self {
        self.value.branch = Some(Some(branch));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "branch", "` request field to JSON null."
    )]
    #[must_use]
    pub fn branch_null(mut self) -> Self {
        self.value.branch = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "branch", "` request field.")]
    #[must_use]
    pub fn branch_absent(mut self) -> Self {
        self.value.branch = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "brief", "` request field to a value."
    )]
    #[must_use]
    pub fn brief(mut self, brief: String) -> Self {
        self.value.brief = Some(Some(brief));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "brief", "` request field to JSON null."
    )]
    #[must_use]
    pub fn brief_null(mut self) -> Self {
        self.value.brief = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "brief", "` request field.")]
    #[must_use]
    pub fn brief_absent(mut self) -> Self {
        self.value.brief = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "continues", "` request field to a value."
    )]
    #[must_use]
    pub fn continues(mut self, continues: String) -> Self {
        self.value.continues = Some(Some(continues));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "continues", "` request field to JSON null."
    )]
    #[must_use]
    pub fn continues_null(mut self) -> Self {
        self.value.continues = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "continues", "` request field.")]
    #[must_use]
    pub fn continues_absent(mut self) -> Self {
        self.value.continues = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "depends_on", "` request field to a value."
    )]
    #[must_use]
    pub fn depends_on(mut self, depends_on: Vec<String>) -> Self {
        self.value.depends_on = Some(Some(depends_on));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "depends_on", "` request field to JSON null."
    )]
    #[must_use]
    pub fn depends_on_null(mut self) -> Self {
        self.value.depends_on = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "depends_on", "` request field.")]
    #[must_use]
    pub fn depends_on_absent(mut self) -> Self {
        self.value.depends_on = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "mode", "` request field to a value."
    )]
    #[must_use]
    pub fn mode(mut self, mode: String) -> Self {
        self.value.mode = Some(Some(mode));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "mode", "` request field to JSON null."
    )]
    #[must_use]
    pub fn mode_null(mut self) -> Self {
        self.value.mode = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "mode", "` request field.")]
    #[must_use]
    pub fn mode_absent(mut self) -> Self {
        self.value.mode = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "model", "` request field to a value."
    )]
    #[must_use]
    pub fn model(mut self, model: String) -> Self {
        self.value.model = Some(Some(model));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "model", "` request field to JSON null."
    )]
    #[must_use]
    pub fn model_null(mut self) -> Self {
        self.value.model = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "model", "` request field.")]
    #[must_use]
    pub fn model_absent(mut self) -> Self {
        self.value.model = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "participant", "` request field to a value."
    )]
    #[must_use]
    pub fn participant(mut self, participant: String) -> Self {
        self.value.participant = Some(Some(participant));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "participant", "` request field to JSON null."
    )]
    #[must_use]
    pub fn participant_null(mut self) -> Self {
        self.value.participant = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "participant", "` request field.")]
    #[must_use]
    pub fn participant_absent(mut self) -> Self {
        self.value.participant = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "profile", "` request field to a value."
    )]
    #[must_use]
    pub fn profile(mut self, profile: String) -> Self {
        self.value.profile = Some(Some(profile));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "profile", "` request field to JSON null."
    )]
    #[must_use]
    pub fn profile_null(mut self) -> Self {
        self.value.profile = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "profile", "` request field.")]
    #[must_use]
    pub fn profile_absent(mut self) -> Self {
        self.value.profile = None;
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "thought_level", "` request field to a value."
    )]
    #[must_use]
    pub fn thought_level(mut self, thought_level: String) -> Self {
        self.value.thought_level = Some(Some(thought_level));
        self
    }
    #[doc = concat!(
        "Set the optional nullable `", "thought_level", "` request field to JSON null."
    )]
    #[must_use]
    pub fn thought_level_null(mut self) -> Self {
        self.value.thought_level = Some(None);
        self
    }
    #[doc = concat!("Omit the optional nullable `", "thought_level", "` request field.")]
    #[must_use]
    pub fn thought_level_absent(mut self) -> Self {
        self.value.thought_level = None;
        self
    }
    /// Finish building the request model.
    pub fn build(self) -> WorkspaceDeclaration {
        self.value
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceMessage {
    pub message: String,
    ///The person posting, by declared name. Trimmed, 1 to 64 characters with no control characters, and never the name of an Agent in the Organization.
    pub participant: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceMessageWithdrawal {
    ///The person who wrote the message, by declared name. Trimmed, 1 to 64 characters with no control characters, and never the name of an Agent in the Organization.
    pub participant: String,
}
