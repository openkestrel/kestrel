use std::str::FromStr;

use anyhow::{Result, bail};

macro_rules! vocabulary {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl FromStr for $name {
            type Err = anyhow::Error;

            fn from_str(text: &str) -> Result<Self> {
                match text {
                    $($text => Ok($name::$variant),)+
                    other => bail!("{other} is not a {}", stringify!($name)),
                }
            }
        }
    };
}

vocabulary!(State {
    Unchecked => "unchecked",
    LoginCompleted => "login_completed",
    CredentialAccepted => "credential_accepted",
    AuthenticationFailed => "authentication_failed",
    Expired => "expired",
    NotCovered => "not_covered",
});

vocabulary!(Source {
    Import => "import",
    ProviderCheck => "provider_check",
    Relay => "relay",
    GenericWrite => "generic_write",
    Refresh => "refresh",
    Session => "session",
    ModelTest => "model_test",
});

vocabulary!(UseResult {
    Worked => "worked",
    AuthenticationFailed => "authentication_failed",
    NotCovered => "not_covered",
});

vocabulary!(UseSource {
    ModelTest => "model_test",
    Session => "session",
});

vocabulary!(TestState {
    Pending => "pending",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Interrupted => "interrupted",
});

vocabulary!(TestOutcome {
    Worked => "worked",
    AuthenticationFailed => "authentication_failed",
    NotCovered => "not_covered",
    Unavailable => "unavailable",
    HarnessFailed => "harness_failed",
    MaterialReplaced => "material_replaced",
});
