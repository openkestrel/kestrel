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
