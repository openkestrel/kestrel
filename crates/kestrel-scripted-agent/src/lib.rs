//! The scripts the agent plays, shared with the harness that selects one.

use clap::ValueEnum;

/// The prefix of every environment variable the `Confides` script says it can see. No provider
/// names one this way, so a real key in the environment the suite runs in cannot be said by
/// accident.
pub const CONFIDED: &str = "SCRIPTED_";

/// Where the `Refreshes` script keeps its login, beneath its home.
pub const LOGIN: &str = ".scripted/login";
/// What the `Refreshes` script appends to the login it found, standing in for a new token.
pub const REFRESHED: &str = " refreshed";

/// The two models the agent offers a client, the first of which it runs on unasked.
pub const DEFAULT_MODEL: &str = "scripted-mini";
pub const OTHER_MODEL: &str = "scripted-max";
/// What the `Mutters` script writes to stderr, followed by a line of `OVERLONG` bytes.
pub const MUTTERED: &str = "the scripted agent muttered to itself";
pub const OVERLONG: usize = 64 * 1024;
pub const CHATTER: std::time::Duration = std::time::Duration::from_millis(100);
pub const CHATTERED_LINES: usize = 10;
pub const CHATTERED_MESSAGES: usize = 5;
pub const FIRST_MEMORY: &str = "the first remembered message";
pub const LAST_MEMORY: &str = "the last remembered message";

/// What the `Announces` script calls its conversation, and the command it offers for it.
pub const TITLE: &str = "the scripted conversation";
pub const COMMAND: &str = "compact";
pub const COMMAND_DESCRIPTION: &str = "Compact the conversation";
pub const COMMAND_HINT: &str = "/compact";
/// The option the `Announces` script reports in a category kestrel does not know, whose name
/// begins with `_` as ACP reserves.
pub const CUSTOM_OPTION: &str = "verbose";
pub const CUSTOM_CATEGORY: &str = "_scripted";
/// The Mode-category option a scripted agent offers, and the mode it starts on.
pub const MODE_OPTION: &str = "mode";
pub const STARTING_MODE: &str = "build";
pub const SWITCHED_MODE: &str = "plan";
/// The second mode a legacy-modes script offers beside the one it is on.
pub const OTHER_MODE: &str = "review";
/// What the `Repeats` script sends, identically, over and over inside one turn.
pub const REPEATS: usize = 20;
/// What the `BurstsUsage` script reports it has spent: one burst whose last value is
/// [`BURSTED_USAGE`], inside a context of [`BURSTED_SIZE`] tokens.
pub const BURSTED_USAGE: u64 = 400;
pub const BURSTED_SIZE: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Script {
    /// Plans, thinks, calls a tool it asks permission for, says two messages and ends the turn.
    Speaks,
    SlowTool,
    OversizedTool,
    ReconnectingTools,
    OpenToolAnswered,
    OpenToolCancelled,
    OpenToolFailed,
    ReportsThenWaits,
    CancelledText,
    FailedText,
    /// Says one thing and ends the turn without having finished.
    Refuses,
    /// Says which Provider Credentials reached its own process, and nothing else.
    Confides,
    InspectsEnvironment,
    /// Says the login it found beneath its home, and rewrites it there as a harness refreshing
    /// one does.
    Refreshes,
    /// Says whether both ends of a long earlier context reached its prompt.
    Recalls,
    /// Says back exactly the prompt it was sent.
    Echoes,
    /// Says which turn of its one session each prompt is, and every prompt that came before it
    /// there, which only a conversation that went on can know.
    Converses,
    /// Dies mid-turn without answering the prompt.
    Dies,
    /// Answers `initialize` with a protocol version it was not asked for.
    Predates,
    /// Answers `initialize` offering only a terminal to log in at.
    Demands,
    /// Offers a client no say in which model it runs on.
    Decides,
    /// Will not open a session for a client that has not logged in.
    Insists,
    /// Works at a turn that never ends, so nothing the agent does is what ends the Session.
    Dawdles,
    Writes,
    /// Writes to stderr as it works at a turn that never ends.
    Mutters,
    /// Writes to stderr several times a second through each turn, as a harness running `git` does.
    Chatters,
    /// Speaks, but takes long enough over the turn that the control plane can be killed and
    /// restarted while the Session is still in flight.
    Lingers,
    /// Says the directory its session was opened against, and nothing else.
    Locates,
    /// Ends the turn without a message, a thought, a plan or a tool call, having sent only the
    /// bookkeeping a harness sends when its agent says nothing.
    Silent,
    /// Calls a tool and ends the turn, without saying anything.
    Works,
    /// Asks permission and ends the turn, without a message, a thought, a plan or a tool call
    /// notification of its own.
    Asks,
    /// Answers its first turn, then produces nothing on the turns after it.
    Lapses,
    /// Converses, keeping its session on disk where a new process can load it, and dies the first
    /// time it is prompted for a second turn.
    Revives,
    /// Answers its turn, then exits between turns with nothing asked of it.
    Vanishes,
    /// Says its title, offers commands and reports its config options with a Mode-category
    /// option and a `_`-prefixed one, then switches its own mode mid-turn.
    Announces,
    /// Sends the same config options, identically, over and over inside one turn.
    Repeats,
    /// Switches its own model mid-turn, as an agent asked to change model does.
    SwitchesModel,
    /// Offers only legacy `modes` and no Mode-category option, and switches mode mid-turn.
    LegacyModes,
    /// Offers only legacy `modes` and no Mode-category option, and keeps the mode it is set to.
    LegacyModesKept,
    /// Reports what it has spent several times in one burst, then answers the turn.
    BurstsUsage,
}

impl Script {
    pub const fn as_str(self) -> &'static str {
        match self {
            Script::Speaks => "speaks",
            Script::ReconnectingTools => "reconnecting-tools",
            Script::OversizedTool => "oversized-tool",
            Script::SlowTool => "slow-tool",
            Script::OpenToolAnswered => "open-tool-answered",
            Script::OpenToolCancelled => "open-tool-cancelled",
            Script::OpenToolFailed => "open-tool-failed",
            Script::ReportsThenWaits => "reports-then-waits",
            Script::CancelledText => "cancelled-text",
            Script::FailedText => "failed-text",
            Script::Refuses => "refuses",
            Script::Confides => "confides",
            Script::InspectsEnvironment => "inspects-environment",
            Script::Refreshes => "refreshes",
            Script::Recalls => "recalls",
            Script::Echoes => "echoes",
            Script::Converses => "converses",
            Script::Dies => "dies",
            Script::Predates => "predates",
            Script::Demands => "demands",
            Script::Decides => "decides",
            Script::Insists => "insists",
            Script::Dawdles => "dawdles",
            Script::Writes => "writes",
            Script::Mutters => "mutters",
            Script::Chatters => "chatters",
            Script::Lingers => "lingers",
            Script::Locates => "locates",
            Script::Silent => "silent",
            Script::Works => "works",
            Script::Asks => "asks",
            Script::Lapses => "lapses",
            Script::Revives => "revives",
            Script::Vanishes => "vanishes",
            Script::Announces => "announces",
            Script::Repeats => "repeats",
            Script::SwitchesModel => "switches-model",
            Script::LegacyModes => "legacy-modes",
            Script::LegacyModesKept => "legacy-modes-kept",
            Script::BurstsUsage => "bursts-usage",
        }
    }
}

pub fn chattered(turn: usize, message: usize) -> String {
    format!("turn {turn}, message {message}")
}

/// What the `Converses` script says to the `turn`th prompt of its session, after `earlier`.
pub fn conversed(turn: usize, earlier: &[String]) -> String {
    format!("turn {turn}, after: {}", earlier.join(" | "))
}
