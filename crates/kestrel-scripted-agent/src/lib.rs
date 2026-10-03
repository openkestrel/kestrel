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

pub const TITLE: &str = "the scripted conversation";
pub const COMMAND: &str = "compact";
pub const COMMAND_DESCRIPTION: &str = "Compact the conversation";
pub const COMMAND_HINT: &str = "/compact";
pub const CUSTOM_OPTION: &str = "verbose";
pub const CUSTOM_CATEGORY: &str = "_scripted";
pub const MODE_OPTION: &str = "mode";
pub const STARTING_MODE: &str = "build";
pub const SWITCHED_MODE: &str = "plan";
pub const THOUGHT_LEVEL_OPTION: &str = "thinking";
pub const STARTING_THOUGHT_LEVEL: &str = "low";
pub const OTHER_THOUGHT_LEVEL: &str = "high";
pub const OTHER_MODE: &str = "review";
pub const REPEATS: usize = 20;
pub const BURSTED_USAGE: u64 = 400;
pub const BURSTED_SIZE: u64 = 1_000;
/// How long a unit an agent runs after answering stays open.
pub const BACKGROUND: std::time::Duration = std::time::Duration::from_secs(3);
pub const BOOKKEEPING: std::time::Duration = std::time::Duration::from_secs(6);
/// Longer than a test's quiet period, so the Session is waiting before its agent resumes.
pub const RESUMES_AFTER: std::time::Duration = std::time::Duration::from_secs(4);
pub const SAID_WHILE_TRAILING: &str = "the background tests passed";
pub const BACKGROUND_TASK: &str = "cargo test --workspace";
/// Longer than a test's quiet period, and silent throughout.
pub const TASK_RUNS: std::time::Duration = std::time::Duration::from_secs(4);
pub const UNKNOWN_UPDATE: &str = "scripted_mystery";
pub const WRITTEN_WHILE_TRAILING: &str = "written-while-trailing.txt";
pub const CHILD_TITLE: &str = "explore the checkout";
pub const CHILD_SAID_IN_TURN: &str = "the child read the code";
pub const CHILD_SAID_WHILE_TRAILING: &str = "the child finished its search";
/// Longer than a test's quiet period, so only the child still running keeps its Session trailing.
pub const CHILD_SILENT_FOR: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Script {
    /// Plans, thinks, calls a tool it asks permission for, says two messages and ends the turn.
    Speaks,
    SlowTool,
    OversizedTool,
    ReconnectingTools,
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
    Repeats,
    SwitchesModel,
    /// Offers only legacy `modes` and no Mode-category option, and switches mode mid-turn.
    LegacyModes,
    /// Offers only legacy `modes` and no Mode-category option, and keeps the mode it is set to.
    LegacyModesKept,
    BurstsUsage,
    /// Says a message and works at a tool call until the client cancels the turn, then answers
    /// `cancelled` with the tool call still open.
    WorksUntilCancelled,
    AnswersThenWorksUntilCancelled,
    IgnoresCancel,
    RefusesOptions,
    /// Answers, then says `SAID_WHILE_TRAILING` and runs a tool call for `BACKGROUND`, as an agent
    /// waiting on its own background command does.
    AnswersThenWorks,
    /// Answers with a tool call still running, and settles it `BACKGROUND` later.
    AnswersWithAToolOpen,
    /// Answers with a tool call still running, then exits cleanly without settling it.
    AnswersWithAToolOpenThenExits,
    /// Answers with a tool call still running, and settles it `BACKGROUND` into the next turn.
    CarriesAToolOver,
    /// Answers, then sends only usage and config option updates for `BOOKKEEPING`.
    AnswersThenKeepsBooks,
    /// Answers, stays quiet for `RESUMES_AFTER`, then runs a tool call for `BACKGROUND`.
    ResumesAfterSettling,
    /// Answers, then writes `WRITTEN_WHILE_TRAILING` into its checkout.
    AnswersThenWrites,
    /// Sends an `UNKNOWN_UPDATE`, then answers with Claude's async task `BACKGROUND_TASK`, sent
    /// only if the client declared `asyncTasks`, running silently for `TASK_RUNS`.
    AnswersWithABackgroundTask,
    /// Runs an OpenCode child through its turn and silently past the answer for `CHILD_SILENT_FOR`,
    /// sending its output on the child channel only to a client that declared it.
    #[value(name = "runs-an-opencode-child")]
    RunsAnOpenCodeChild,
}

impl Script {
    pub const fn as_str(self) -> &'static str {
        match self {
            Script::Speaks => "speaks",
            Script::ReconnectingTools => "reconnecting-tools",
            Script::OversizedTool => "oversized-tool",
            Script::SlowTool => "slow-tool",
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
            Script::WorksUntilCancelled => "works-until-cancelled",
            Script::AnswersThenWorksUntilCancelled => "answers-then-works-until-cancelled",
            Script::IgnoresCancel => "ignores-cancel",
            Script::RefusesOptions => "refuses-options",
            Script::AnswersThenWorks => "answers-then-works",
            Script::AnswersWithAToolOpen => "answers-with-a-tool-open",
            Script::AnswersWithAToolOpenThenExits => "answers-with-a-tool-open-then-exits",
            Script::CarriesAToolOver => "carries-a-tool-over",
            Script::AnswersThenKeepsBooks => "answers-then-keeps-books",
            Script::ResumesAfterSettling => "resumes-after-settling",
            Script::AnswersThenWrites => "answers-then-writes",
            Script::AnswersWithABackgroundTask => "answers-with-a-background-task",
            Script::RunsAnOpenCodeChild => "runs-an-opencode-child",
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
