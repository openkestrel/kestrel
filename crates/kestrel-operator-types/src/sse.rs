// Generated from openapi/operator.json by the generate binary.
use crate::*;
impl ChangesEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Change(..) => "change",
            Self::ChangesOpen(..) => "open",
            Self::ChangesResync(..) => "resync",
        }
    }
    pub fn from_sse(event: &str, data: &str) -> Option<Result<Self, serde_json::Error>> {
        Some(match event {
            "change" => serde_json::from_str(data).map(Self::Change),
            "open" => serde_json::from_str(data).map(Self::ChangesOpen),
            "resync" => serde_json::from_str(data).map(Self::ChangesResync),
            _ => return None,
        })
    }
}
impl Event {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Activity(..) => "activity",
            Self::TranscriptSessionState(..) => "session_state",
            Self::Recorded(..) => "entry",
            Self::End(..) => "end",
            Self::CursorEvent(..) => "cursor",
            Self::FollowerEvent(..) => "follower",
            Self::Presence(..) => "presence",
        }
    }
    pub fn from_sse(event: &str, data: &str) -> Option<Result<Self, serde_json::Error>> {
        Some(match event {
            "activity" => serde_json::from_str(data).map(Self::Activity),
            "session_state" => serde_json::from_str(data).map(Self::TranscriptSessionState),
            "entry" => serde_json::from_str(data).map(Self::Recorded),
            "end" => serde_json::from_str(data).map(Self::End),
            "cursor" => serde_json::from_str(data).map(Self::CursorEvent),
            "follower" => serde_json::from_str(data).map(Self::FollowerEvent),
            "presence" => serde_json::from_str(data).map(Self::Presence),
            _ => return None,
        })
    }
}
