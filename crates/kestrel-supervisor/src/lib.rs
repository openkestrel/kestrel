pub mod changes;
pub mod checkout;
pub mod completer;
pub mod files;
pub mod harness;
pub mod link;
pub mod login;
pub mod permission;

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::harness::{Conversation, Harness, Turn};
use crate::link::{Answer, AnswerBody, Checkout, Down, Exit, Instruction, Link, Read, Report};

const RECONNECT_AFTER: Duration = Duration::from_millis(250);
/// Often enough that the control plane keeps its hold on this Instance's Session through a
/// handful of these going missing, and through the control plane itself restarting under it.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(2);
/// Waited past the lease the control plane holds on a Session before letting it go. Longer than a
/// heartbeat, so a reachable link resets the clock between two of them, and long enough that a
/// control plane restarting inside its lease is still met.
const GIVE_UP_MARGIN: Duration = Duration::from_secs(5);
const STDERR_LINES_PER_REPORT: usize = 64;
const STDERR_REPORT_PATIENCE: Duration = Duration::from_secs(5);
/// Trailing edge: a burst collapses into one report a second (ADR-0041).
const SESSION_INFO_EVERY: Duration = Duration::from_secs(1);
const USAGE_EVERY: Duration = Duration::from_secs(1);
/// Long enough for the last lines of an agent that has just exited, which are often why.
const STDERR_DRAINING: Duration = Duration::from_secs(1);
const INTERRUPT_DEADLINE: Duration = Duration::from_secs(30);
const QUIET_PERIOD: Duration = Duration::from_secs(30);

pub trait Diagnostics {
    fn info(&self, message: &str);
}

pub struct Stderr;

impl Diagnostics for Stderr {
    /// A control plane that dies takes the pipe it was reading these over with it, and an
    /// Instance outlives a control-plane restart (ADR-0002) rather than dying into one.
    fn info(&self, message: &str) {
        let _ = writeln!(io::stderr(), "{message}");
    }
}

/// Held across a reconnect.
struct Supervising {
    cursor: Option<String>,
    carrying: Option<Carrying>,
    checkout: Option<Checkout>,
    summary: Option<Vec<link::WorkRepository>>,
    interrupt_deadline: Duration,
    quiet_period: Duration,
}

/// One Session's part of the supervisor's life. The conversation goes on whether or not the link
/// does, and what is left to say about it is what the supervisor comes back to.
struct Carrying {
    session: String,
    checkout: Checkout,
    prompt: Option<Turn>,
    harness: Harness,
    conversation: Option<Conversation>,
    finished: bool,
    working: bool,
    /// Said again after a reconnect, in case the report was lost on its way.
    ready: bool,
    taken: i64,
    state: Option<Report>,
    /// Handed back before anything is said, because saying the Session finished ends it and with it
    /// this Instance's right to hand anything back for it.
    refreshed: BTreeMap<String, String>,
    written: Option<login::Written>,
    saying: VecDeque<Report>,
    info: Option<link::SessionInfo>,
    info_due: Option<tokio::time::Instant>,
    usage: Option<link::Usage>,
    usage_due: Option<tokio::time::Instant>,
    /// What the last usage report said, which a snapshot carries so that a value still inside its
    /// window never reaches a follower ahead of the trailing edge.
    usage_reported: Option<link::Usage>,
}

impl Carrying {
    fn hold(&mut self, info: link::SessionInfo) {
        if self.info.as_ref() == Some(&info) {
            return;
        }
        self.info = Some(info);
        if self.info_due.is_none() {
            self.info_due = Some(tokio::time::Instant::now() + SESSION_INFO_EVERY);
        }
    }

    fn hold_usage(&mut self, usage: link::Usage) {
        if self.usage.as_ref() == Some(&usage) {
            return;
        }
        self.usage = Some(usage);
        if self.usage_due.is_none() {
            self.usage_due = Some(tokio::time::Instant::now() + USAGE_EVERY);
        }
    }
}

impl Carrying {
    /// Ends the harness and takes the Subscription Profile's files with it, so nothing of the
    /// Session's credentials outlives it on the Instance (ADR-0010).
    async fn let_go(self, because: &str, diagnostics: &dyn Diagnostics) {
        diagnostics.info(&format!("let the session {} go: {because}", self.session));
        if let Some(conversation) = self.conversation {
            conversation.end().await;
        }
        if let Some(written) = self.written {
            written.remove();
        }
    }
}

pub async fn run(diagnostics: &dyn Diagnostics, variables: &BTreeMap<String, String>) -> i32 {
    diagnostics.info("supervisor started");

    let Some(link) = dialled(variables) else {
        diagnostics.info(
            "no link to dial: set KESTREL_LINK, KESTREL_INSTANCE and KESTREL_INSTANCE_CREDENTIAL",
        );
        return 1;
    };
    let link = Arc::new(link);
    let (stderr, written) = mpsc::unbounded_channel();
    let home = set(variables, "HOME").map(PathBuf::from);
    let mut supervising = Supervising {
        cursor: set(variables, "KESTREL_INSTRUCTIONS_AFTER").map(str::to_owned),
        carrying: None,
        checkout: None,
        summary: None,
        interrupt_deadline: interrupt_deadline(variables),
        quiet_period: quiet_period(variables),
    };

    let alive = tokio::spawn(saying_it_is_alive(Arc::clone(&link)));
    let mut relaying = tokio::spawn(relaying_stderr(Arc::clone(&link), written));
    let status = supervised(
        &link,
        &stderr,
        home.as_deref(),
        &mut supervising,
        give_up_after(variables),
        diagnostics,
    )
    .await;
    if let Some(carrying) = supervising.carrying.take() {
        carrying
            .let_go("this instance is off the link for good", diagnostics)
            .await;
    }
    alive.abort();
    drop(stderr);
    if tokio::time::timeout(STDERR_DRAINING, &mut relaying)
        .await
        .is_err()
    {
        relaying.abort();
    }

    status
}

/// Folds in the last reported usage, since the snapshot replaces what the control plane holds.
async fn report_state(link: &Link, carrying: &Carrying) -> Result<(), link::Error> {
    let Some(mut state) = carrying.state.clone() else {
        return Ok(());
    };
    if let Report::SessionState { usage, .. } = &mut state {
        usage.clone_from(&carrying.usage_reported);
    }

    match tokio::time::timeout(
        Duration::from_secs(1),
        link.report(&state, Some(&carrying.session), None),
    )
    .await
    {
        Ok(Err(error @ (link::Error::Refused(_) | link::Error::Session(_)))) => Err(error),
        _ => Ok(()),
    }
}

async fn saying_it_is_alive(link: Arc<Link>) {
    loop {
        tokio::time::sleep(HEARTBEAT_EVERY).await;
        // Whether the link is there at all is the attending loop's to notice and reconnect
        // through; this one says what it can, whenever it can.
        let _ = link.report(&Report::Heartbeat, None, None).await;
    }
}

/// A line the link was not there to take in time is lost, never queued behind a reconnect.
async fn relaying_stderr(link: Arc<Link>, mut written: mpsc::UnboundedReceiver<String>) {
    let mut lines = Vec::new();
    while written.recv_many(&mut lines, STDERR_LINES_PER_REPORT).await > 0 {
        let report = Report::Stderr {
            lines: std::mem::take(&mut lines),
        };
        let _ =
            tokio::time::timeout(STDERR_REPORT_PATIENCE, link.report(&report, None, None)).await;
    }
}

/// Redials for as long as the Instance lives: only a link that refuses this Instance, which it
/// does once the Instance has been let go, ends it.
async fn supervised(
    link: &Arc<Link>,
    stderr: &mpsc::UnboundedSender<String>,
    home: Option<&Path>,
    supervising: &mut Supervising,
    give_up_after: Option<Duration>,
    diagnostics: &dyn Diagnostics,
) -> i32 {
    loop {
        match attend(link, stderr, home, supervising, give_up_after, diagnostics).await {
            Ok(()) => diagnostics.info("lost the link"),
            Err(link::Error::Refused(why)) => {
                diagnostics.info(&format!("the link refused this instance: {why}"));
                return 1;
            }
            Err(link::Error::Lost(why) | link::Error::Session(why)) => {
                diagnostics.info(&format!("lost the link: {why}"));
            }
        }

        if lapsed(link, supervising, give_up_after) {
            give_up(link, supervising, diagnostics).await;
        }

        tokio::time::sleep(RECONNECT_AFTER).await;
    }
}

fn lapsed(link: &Link, supervising: &Supervising, give_up_after: Option<Duration>) -> bool {
    supervising.carrying.is_some()
        && give_up_after.is_some_and(|bound| link.unreached_for() >= bound)
}

/// A control plane gone past the lease has already let the Session go, so nothing is waiting for
/// the work its harness would be kept open for. The supervisor stays, for the Instance.
async fn give_up(link: &Link, supervising: &mut Supervising, diagnostics: &dyn Diagnostics) {
    let Some(carrying) = supervising.carrying.take() else {
        return;
    };
    let because = format!(
        "gave up on it, since the link has not answered for {:?}, past the session's lease",
        link.unreached_for()
    );
    carrying.let_go(&because, diagnostics).await;
}

async fn attend(
    link: &Arc<Link>,
    stderr: &mpsc::UnboundedSender<String>,
    home: Option<&Path>,
    supervising: &mut Supervising,
    give_up_after: Option<Duration>,
    diagnostics: &dyn Diagnostics,
) -> Result<(), link::Error> {
    let mut instructions = link.open(supervising.cursor.as_deref()).await?;
    match supervising.cursor.as_deref() {
        None => diagnostics.info("link open"),
        Some(held) => diagnostics.info(&format!("link open after {held}")),
    }

    if let Some(checkout) = link.connected().await? {
        supervising.checkout = Some(checkout);
    }
    if let Some(carrying) = supervising.carrying.as_mut() {
        while let Some(event) = carrying
            .conversation
            .as_mut()
            .and_then(Conversation::try_next)
        {
            match event {
                harness::ConversationEvent::State(state) => carrying.state = Some(state),
                harness::ConversationEvent::Report(Report::Usage { usage }) => {
                    carrying.hold_usage(usage)
                }
                harness::ConversationEvent::Report(Report::SessionInfo(info)) => {
                    carrying.hold(info)
                }
                harness::ConversationEvent::Report(report) => carrying.saying.push_back(report),
                harness::ConversationEvent::Worked(worked) => {
                    carrying.working = false;
                    worked_on(carrying, worked, diagnostics).await;
                }
                harness::ConversationEvent::Interrupted => {
                    carrying.working = false;
                    carrying.saying.push_back(Report::Interrupted);
                }
                harness::ConversationEvent::Settled => settled(carrying).await,
                harness::ConversationEvent::Ready => carrying.ready = true,
            }
        }
    }
    if let Some(carrying) = supervising.carrying.as_ref() {
        let reported = report_state(link, carrying).await;
        let_go_if_refused(reported, supervising, diagnostics).await?;
    }
    if let Some(carrying) = supervising.carrying.as_ref()
        && carrying.state.is_some()
        && carrying.prompt.is_none()
        && carrying.ready
    {
        let reported = link
            .report(&Report::Ready, Some(&carrying.session), None)
            .await;
        let_go_if_refused(reported, supervising, diagnostics).await?;
    }
    report_work(link, supervising, true).await?;
    // The bookkeeping report goes up again: a control plane that restarted under the Instance
    // missed it.
    if let Some(carrying) = supervising.carrying.as_mut() {
        if carrying.info.is_some() {
            carrying.info_due = Some(tokio::time::Instant::now());
        }
        if carrying.usage.is_some() {
            carrying.usage_due = Some(tokio::time::Instant::now());
        }
    }
    let mut checking = tokio::time::interval_at(
        tokio::time::Instant::now() + HEARTBEAT_EVERY,
        HEARTBEAT_EVERY,
    );
    checking.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    diagnostics.info("reported connected");

    loop {
        if let Some(carrying) = supervising.carrying.as_mut() {
            match carried(link, home, carrying, diagnostics).await {
                Ok(true) => {}
                Ok(false) => {
                    if let Some(carrying) = supervising.carrying.take() {
                        carrying.let_go("it is over", diagnostics).await;
                    }
                }
                Err(link::Error::Session(why)) => {
                    if let Some(carrying) = supervising.carrying.take() {
                        carrying.let_go(&why, diagnostics).await;
                    }
                }
                Err(error) => return Err(error),
            }
        }

        let give_up_timer = until_given_up(link, supervising.carrying.is_some(), give_up_after);
        let info_due = supervising
            .carrying
            .as_ref()
            .and_then(|carrying| carrying.info_due);
        let usage_due = supervising
            .carrying
            .as_ref()
            .and_then(|carrying| carrying.usage_due);
        tokio::select! {
            delivered = instructions.next() => {
                let delivered = match delivered? {
                    None => return Ok(()),
                    Some(Down::Read(asked)) => {
                        answer(link, supervising.checkout.as_ref(), asked);
                        continue;
                    }
                    Some(Down::Instruction(delivered)) => delivered,
                };
                supervising.cursor = Some(delivered.id.clone());
                diagnostics.info(&format!(
                    "instruction {} {} for session {}",
                    delivered.instruction.kind(),
                    delivered.id,
                    delivered.session
                ));
                // Any Stop, not only one for the Session still carried: a report refused since the
                // Session ended may already have let it go.
                let stopped = matches!(&delivered.instruction, Instruction::Stop);
                instructed(stderr, supervising, delivered, diagnostics).await;
                if stopped {
                    report_work(link, supervising, true).await?;
                }
            }
            event = next(&mut supervising.carrying) => {
                if let Some(carrying) = supervising.carrying.as_mut() {
                    match event {
                        harness::ConversationEvent::State(state) => {
                            carrying.state = Some(state);
                            let reported = report_state(link, carrying).await;
                            let_go_if_refused(reported, supervising, diagnostics).await?;
                        }
                        harness::ConversationEvent::Report(Report::Usage { usage }) => {
                            carrying.hold_usage(usage);
                        }
                        harness::ConversationEvent::Report(Report::SessionInfo(info)) => {
                            carrying.hold(info);
                        }
                        harness::ConversationEvent::Report(report) => carrying.saying.push_back(report),
                        harness::ConversationEvent::Worked(worked) => {
                            carrying.working = false;
                            worked_on(carrying, worked, diagnostics).await;
                            report_work(link, supervising, true).await?;
                        }
                        harness::ConversationEvent::Interrupted => {
                            carrying.working = false;
                            carrying.saying.push_back(Report::Interrupted);
                            report_work(link, supervising, true).await?;
                        }
                        harness::ConversationEvent::Settled => {
                            settled(carrying).await;
                            report_work(link, supervising, true).await?;
                        }
                        harness::ConversationEvent::Ready => {
                            let reported = ready(link, carrying).await;
                            let_go_if_refused(reported, supervising, diagnostics).await?;
                        }
                    }
                }
            }
            _ = checking.tick() => {
                if let Some(carrying) = &supervising.carrying {
                    let reported = report_state(link, carrying).await;
                    let_go_if_refused(reported, supervising, diagnostics).await?;
                }
                if supervising.carrying.as_ref().is_some_and(|carrying| carrying.working) {
                    report_work(link, supervising, false).await?;
                }
            }
            _ = until_session_info_due(info_due) => {
                if let Some(carrying) = supervising.carrying.as_mut() {
                    let reported = say_session_info(link, carrying, diagnostics).await;
                    let_go_if_refused(reported, supervising, diagnostics).await?;
                }
            }
            _ = until_usage_due(usage_due) => {
                if let Some(carrying) = supervising.carrying.as_mut() {
                    let reported = say_usage(link, carrying, diagnostics).await;
                    let_go_if_refused(reported, supervising, diagnostics).await?;
                }
            }
            () = give_up_timer => {
                // Re-checked here because a heartbeat may have reached the link since this was
                // armed; a reachable link is not one to give up on.
                if lapsed(link, supervising, give_up_after) {
                    give_up(link, supervising, diagnostics).await;
                }
            }
        }
    }
}

async fn let_go_if_refused(
    reported: Result<(), link::Error>,
    supervising: &mut Supervising,
    diagnostics: &dyn Diagnostics,
) -> Result<(), link::Error> {
    match reported {
        Err(link::Error::Session(why)) => {
            if let Some(carrying) = supervising.carrying.take() {
                carrying.let_go(&why, diagnostics).await;
            }
            Ok(())
        }
        reported => reported,
    }
}

async fn ready(link: &Link, carrying: &mut Carrying) -> Result<(), link::Error> {
    if carrying.prompt.is_none() {
        carrying.ready = true;
        link.report(&Report::Ready, Some(&carrying.session), None)
            .await?;
    }

    Ok(())
}

/// Answered beside whatever the Session is doing, so a read never waits on a turn.
fn answer(link: &Arc<Link>, checkout: Option<&Checkout>, asked: link::Asked) {
    let link = Arc::clone(link);
    let checkouts = files::Checkouts::of(checkout);
    let checkout = checkout.cloned();
    tokio::spawn(async move {
        let answered = match asked.read {
            Read::Files { path } => {
                AnswerBody::Json(files::list(&checkouts, path.as_deref()).await)
            }
            Read::File { path, raw } => files::read(&checkouts, &path, raw).await,
            read @ (Read::Changes { .. } | Read::Commits | Read::Stashes) => {
                AnswerBody::Json(changes::read(&checkouts, checkout.as_ref(), read).await)
            }
            Read::Unrecognized => AnswerBody::Json(Answer::Refused {
                message: "this Instance's supervisor does not know that read".to_owned(),
            }),
        };
        // An answer the link does not take is one the operator has already given up on.
        let _ = link.answer(&asked.request, answered).await;
    });
}

async fn report_work(
    link: &Link,
    supervising: &mut Supervising,
    force: bool,
) -> Result<(), link::Error> {
    let Some(checkout) = &supervising.checkout else {
        return Ok(());
    };
    let repositories = checkout::work(checkout).await;
    if force || supervising.summary.as_ref() != Some(&repositories) {
        link.report(
            &Report::Work {
                repositories: repositories.clone(),
            },
            None,
            None,
        )
        .await?;
        supervising.summary = Some(repositories);
    }
    Ok(())
}

/// An unchanged state is still sent after a reconnect: the control plane may have missed the
/// change.
async fn say_session_info(
    link: &Link,
    carrying: &mut Carrying,
    diagnostics: &dyn Diagnostics,
) -> Result<(), link::Error> {
    let Some(info) = carrying.info.clone() else {
        return Ok(());
    };
    if carrying
        .info_due
        .is_some_and(|due| tokio::time::Instant::now() < due)
    {
        return Ok(());
    }
    link.report(&Report::SessionInfo(info), Some(&carrying.session), None)
        .await?;
    carrying.info_due = None;
    diagnostics.info(&format!("reported session_info for {}", carrying.session));

    Ok(())
}

async fn until_session_info_due(due: Option<tokio::time::Instant>) {
    match due {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending().await,
    }
}

async fn say_usage(
    link: &Link,
    carrying: &mut Carrying,
    diagnostics: &dyn Diagnostics,
) -> Result<(), link::Error> {
    let Some(usage) = carrying.usage.clone() else {
        return Ok(());
    };
    if carrying
        .usage_due
        .is_some_and(|due| tokio::time::Instant::now() < due)
    {
        return Ok(());
    }
    link.report(
        &Report::Usage {
            usage: usage.clone(),
        },
        Some(&carrying.session),
        None,
    )
    .await?;
    carrying.usage_reported = Some(usage);
    carrying.usage_due = None;
    diagnostics.info(&format!("reported usage for {}", carrying.session));

    Ok(())
}

async fn until_usage_due(due: Option<tokio::time::Instant>) {
    match due {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending().await,
    }
}

/// Everything left to do for the Session before waiting on the link again: `false` once it has
/// said all it has to say about a Session that is over.
async fn carried(
    link: &Link,
    home: Option<&Path>,
    carrying: &mut Carrying,
    diagnostics: &dyn Diagnostics,
) -> Result<bool, link::Error> {
    if !carrying.refreshed.is_empty() {
        link.refresh(&carrying.session, &carrying.refreshed).await?;
        diagnostics.info(&format!(
            "handed back {}",
            carrying
                .refreshed
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
        carrying.refreshed.clear();
    }
    say(link, carrying, diagnostics).await?;
    say_session_info(link, carrying, diagnostics).await?;
    say_usage(link, carrying, diagnostics).await?;
    if carrying.finished {
        return Ok(false);
    }
    if carrying.conversation.is_none() {
        carrying.conversation = conversation(link, home, carrying, diagnostics).await?;
    }

    Ok(true)
}

async fn instructed(
    stderr: &mpsc::UnboundedSender<String>,
    supervising: &mut Supervising,
    delivered: link::Delivered,
    diagnostics: &dyn Diagnostics,
) {
    let carrying_it = supervising
        .carrying
        .as_ref()
        .is_some_and(|carrying| carrying.session == delivered.session);

    match delivered.instruction {
        // Stopped is told after the session has already ended, so whatever a turn last refreshed
        // was already handed back before this arrived; only the local copy is left to clean up.
        Instruction::Stop if carrying_it => {
            if let Some(carrying) = supervising.carrying.take() {
                carrying.let_go("it was stopped", diagnostics).await;
            }
        }
        Instruction::Start {
            checkout,
            turn,
            prompt,
            harness,
        } if !carrying_it => {
            start_carrying(
                stderr,
                supervising,
                diagnostics,
                delivered.session,
                checkout,
                Some(Turn { seq: turn, prompt }),
                harness,
            )
            .await;
        }
        Instruction::Unbriefed { checkout, harness } if !carrying_it => {
            start_carrying(
                stderr,
                supervising,
                diagnostics,
                delivered.session,
                checkout,
                None,
                harness,
            )
            .await;
        }
        Instruction::Prompt { turn, prompt } if carrying_it => {
            let prompt = Turn { seq: turn, prompt };
            if let Some(carrying) = supervising.carrying.as_mut() {
                carrying.working = true;
                // An unbriefed Session's first Prompt is its Brief: it has started now.
                if carrying.prompt.is_none() {
                    carrying.prompt = Some(prompt.clone());
                    carrying.saying.push_back(Report::Started);
                }
            }
            match supervising
                .carrying
                .as_ref()
                .and_then(|carrying| carrying.conversation.as_ref())
            {
                Some(conversation) => conversation.prompt(prompt),
                None => diagnostics.info("prompted before the session started"),
            }
        }
        Instruction::Interrupt { turn } if carrying_it => match supervising.carrying.as_ref() {
            Some(carrying) if carrying.working => match carrying.conversation.as_ref() {
                Some(conversation) => conversation.interrupt(turn),
                None => diagnostics.info("interrupted before the session started"),
            },
            _ => diagnostics.info("interrupted a session with no turn in flight"),
        },
        Instruction::SetOption {
            option,
            value,
            participant,
        } if carrying_it => {
            let outcome = match supervising
                .carrying
                .as_mut()
                .and_then(|carrying| carrying.conversation.as_mut())
            {
                Some(conversation) => conversation.set_option(option.clone(), value.clone()).await,
                None => harness::SetOption::refused(
                    option,
                    String::new(),
                    None,
                    "the session's harness is not open".to_owned(),
                ),
            };
            diagnostics.info(&format!(
                "changed {} for {}: {}",
                outcome.option,
                participant,
                outcome
                    .refused
                    .as_deref()
                    .unwrap_or(outcome.to.as_deref().unwrap_or("no value"))
            ));
            if let Some(carrying) = supervising.carrying.as_mut() {
                carrying.saying.push_back(Report::OptionChanged {
                    participant,
                    option: outcome.option,
                    category: outcome.category,
                    from: outcome.from,
                    to: outcome.to,
                    refused: outcome.refused,
                    options: outcome.options,
                });
            }
        }
        Instruction::Stop
        | Instruction::Start { .. }
        | Instruction::Unbriefed { .. }
        | Instruction::Prompt { .. }
        | Instruction::Interrupt { .. }
        | Instruction::SetOption { .. }
        | Instruction::Unrecognized => {}
    }
}

async fn start_carrying(
    stderr: &mpsc::UnboundedSender<String>,
    supervising: &mut Supervising,
    diagnostics: &dyn Diagnostics,
    session: String,
    checkout: Checkout,
    prompt: Option<Turn>,
    harness: link::Harness,
) {
    // A Session's start follows the last one's stop down the stream, so one still carried here is
    // over.
    if let Some(carrying) = supervising.carrying.take() {
        carrying
            .let_go("another session started", diagnostics)
            .await;
    }
    let working = prompt.is_some();
    supervising.checkout = Some(checkout.clone());
    let mut carrying = Carrying {
        session,
        checkout,
        prompt,
        harness: Harness {
            command: harness.command,
            auth: harness.auth,
            model: harness.model,
            mode: harness.mode,
            thought_level: harness.thought_level,
            interrupt_deadline: supervising.interrupt_deadline,
            quiet_period: supervising.quiet_period,
            stderr: stderr.clone(),
        },
        conversation: None,
        finished: false,
        working,
        ready: false,
        taken: 0,
        state: Some(Report::SessionState {
            tools: Vec::new(),
            units: Vec::new(),
            message_buffering: false,
            thought_buffering: false,
            usage: None,
            last_activity_at: None,
        }),
        refreshed: BTreeMap::new(),
        written: None,
        saying: VecDeque::new(),
        info: None,
        info_due: None,
        usage: None,
        usage_due: None,
        usage_reported: None,
    };
    match checkout::check_out(&carrying.checkout).await {
        Ok(()) => {
            if carrying.prompt.is_some() {
                carrying.saying.push_back(Report::Started);
            } else {
                // The checkout is what decides whether the Instance holds unpublished work, and
                // this Session may seal without ever running a turn.
                carrying.saying.push_back(Report::Checkout {
                    repositories: checkout::observe(&carrying.checkout).await,
                });
            }
        }
        Err(because) => {
            diagnostics.info(&because);
            carrying.finished = true;
            carrying.saying.push_back(Report::Checkout {
                repositories: checkout::observe(&carrying.checkout).await,
            });
            carrying.saying.push_back(Report::Finished {
                exit: Exit::Failed { because },
                usage: carrying.usage.clone(),
            });
        }
    }
    supervising.carrying = Some(carrying);
}

async fn worked_on(
    carrying: &mut Carrying,
    worked: harness::Worked,
    diagnostics: &dyn Diagnostics,
) {
    if let Some(on) = &worked.on {
        diagnostics.info(&format!("on the model {}", on.model));
    }
    for subject in &worked.allowed {
        diagnostics.info(&format!("allowed once  {subject}"));
    }
    // Handed back after every turn, not only a finishing one: a login the harness rotates
    // mid-conversation is refreshed while the session is still carried, not saved up for a Stop
    // that arrives once it is not.
    if let Some(written) = &carrying.written {
        carrying.refreshed = written.refreshed();
    }
    if worked.failed.is_some() {
        carrying.finished = true;
        if let Some(conversation) = carrying.conversation.take() {
            conversation.end().await;
        }
        if let Some(written) = carrying.written.take() {
            written.remove();
        }
    }
    // Reported after every turn, not only a finishing one: a Session waiting between turns may be
    // stopped at any moment, and what it last observed is what decides whether its Instance is
    // held.
    let observed = checkout::observe(&carrying.checkout).await;
    carrying
        .saying
        .extend(everything_left_to_say(worked, observed));
}

/// The checkout is observed again because what the agent did while trailing decides whether the
/// Instance holds Unpublished Work.
async fn settled(carrying: &mut Carrying) {
    let observed = checkout::observe(&carrying.checkout).await;
    carrying.saying.push_back(Report::Checkout {
        repositories: observed,
    });
    carrying.saying.push_back(Report::Settled);
}

/// Waits out whatever is left of the bound since the link last answered, so a stream that stays
/// open while nothing answers does not hold a Session's harness past its lease.
async fn until_given_up(link: &Link, carrying: bool, give_up_after: Option<Duration>) {
    match give_up_after {
        Some(bound) if carrying => {
            tokio::time::sleep(bound.saturating_sub(link.unreached_for())).await;
        }
        _ => std::future::pending().await,
    }
}

async fn conversation(
    link: &Link,
    home: Option<&Path>,
    carrying: &mut Carrying,
    diagnostics: &dyn Diagnostics,
) -> Result<Option<Conversation>, link::Error> {
    let credentials = link.credentials(&carrying.session).await?;
    let provider = credentials.variables;
    if !provider.is_empty() {
        diagnostics.info(&format!(
            "carrying {} into the harness",
            provider.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }

    match written(home, credentials.files, diagnostics) {
        Ok(written) => {
            carrying.written = written;
            Ok(Some(Conversation::open(
                &carrying.harness,
                provider,
                carrying.prompt.clone(),
                checkout::root(Some(&carrying.checkout)),
            )))
        }
        Err(because) => {
            diagnostics.info(&because);
            carrying.finished = true;
            carrying.saying.push_back(Report::Finished {
                exit: Exit::Failed { because },
                usage: carrying.usage.clone(),
            });
            Ok(None)
        }
    }
}

async fn next(carrying: &mut Option<Carrying>) -> harness::ConversationEvent {
    match carrying
        .as_mut()
        .and_then(|carrying| carrying.conversation.as_mut())
    {
        Some(conversation) => conversation.next().await,
        None => std::future::pending().await,
    }
}

fn written(
    home: Option<&Path>,
    files: BTreeMap<String, String>,
    diagnostics: &dyn Diagnostics,
) -> Result<Option<login::Written>, String> {
    if files.is_empty() {
        return Ok(None);
    }
    let Some(home) = home else {
        return Err(
            "the session's subscription profile holds files, and this environment has no home to \
             put them in"
                .to_owned(),
        );
    };

    let written = login::write(home, files).map_err(|error| {
        format!("the subscription profile's files could not be written: {error}")
    })?;
    diagnostics.info(&format!(
        "writing {} beneath the agent's home",
        written.paths().collect::<Vec<_>>().join(", ")
    ));

    Ok(Some(written))
}

/// Numbered from the last one the link took, and dropped once it has been taken: a reconnect
/// says only what is left, and a replay carries the number the attempt that was lost carried.
async fn say(
    link: &Link,
    carrying: &mut Carrying,
    diagnostics: &dyn Diagnostics,
) -> Result<(), link::Error> {
    while let Some(report) = carrying.saying.front() {
        let seq = carrying.taken + 1;
        link.report(report, Some(&carrying.session), Some(seq))
            .await?;
        let kind = report.kind();
        carrying.taken = seq;
        carrying.saying.pop_front();
        diagnostics.info(&format!("reported {kind} {seq}"));
    }

    Ok(())
}

fn everything_left_to_say(
    worked: harness::Worked,
    observed: Vec<link::Observed>,
) -> impl Iterator<Item = Report> {
    let harness::Worked { failed, usage, .. } = worked;
    std::iter::once(Report::Checkout {
        repositories: observed,
    })
    .chain(std::iter::once(match failed {
        Some(because) => Report::Finished {
            exit: Exit::Failed { because },
            usage,
        },
        None => Report::Answered { usage },
    }))
}

fn dialled(variables: &BTreeMap<String, String>) -> Option<Link> {
    let base = set(variables, "KESTREL_LINK")?;
    let instance = set(variables, "KESTREL_INSTANCE")?;
    let credential = set(variables, "KESTREL_INSTANCE_CREDENTIAL")?;

    Some(Link::to(base, instance, credential))
}

fn interrupt_deadline(variables: &BTreeMap<String, String>) -> Duration {
    set(variables, "KESTREL_INTERRUPT_DEADLINE")
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map_or(INTERRUPT_DEADLINE, Duration::from_secs)
}

fn quiet_period(variables: &BTreeMap<String, String>) -> Duration {
    set(variables, "KESTREL_QUIET_PERIOD")
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map_or(QUIET_PERIOD, Duration::from_secs)
}

/// How long the control plane holds a Session's lease out, in seconds, as it told this Instance
/// when it started its supervisor. A supervisor handed no lease has no bound to derive and keeps
/// every Session's harness through any outage until it is stopped.
fn give_up_after(variables: &BTreeMap<String, String>) -> Option<Duration> {
    let lease = set(variables, "KESTREL_LEASE")?.parse::<u64>().ok()?;

    Duration::from_secs(lease).checked_add(GIVE_UP_MARGIN)
}

fn set<'a>(variables: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    variables
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Recorder(Mutex<Vec<String>>);

    impl Recorder {
        fn new() -> Self {
            Self(Mutex::new(Vec::new()))
        }

        fn everything_it_said(&self) -> String {
            self.0
                .lock()
                .expect("the recorder should not be poisoned")
                .join("\n")
        }
    }

    impl Diagnostics for Recorder {
        fn info(&self, message: &str) {
            self.0
                .lock()
                .expect("the recorder should not be poisoned")
                .push(message.to_owned());
        }
    }

    fn variables(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[tokio::test]
    async fn the_supervisor_says_when_it_started() {
        let diagnostics = Recorder::new();

        run(&diagnostics, &variables(&[])).await;

        assert!(
            diagnostics
                .everything_it_said()
                .contains("supervisor started")
        );
    }

    #[tokio::test]
    async fn the_supervisor_fails_when_it_is_given_no_link_to_dial() {
        let diagnostics = Recorder::new();

        assert_eq!(run(&diagnostics, &variables(&[])).await, 1);
        assert!(diagnostics.everything_it_said().contains("no link to dial"));
    }

    #[tokio::test]
    async fn a_variable_set_to_nothing_is_not_a_link_to_dial() {
        let diagnostics = Recorder::new();

        let status = run(
            &diagnostics,
            &variables(&[
                ("KESTREL_LINK", ""),
                (
                    "KESTREL_INSTANCE",
                    "local-exec/kestrel-01999cf2-0000-7000-8000-000000000000",
                ),
                ("KESTREL_INSTANCE_CREDENTIAL", "a-credential"),
            ]),
        )
        .await;

        assert_eq!(status, 1);
        assert!(diagnostics.everything_it_said().contains("no link to dial"));
    }

    #[tokio::test]
    async fn the_supervisor_fails_when_it_is_given_a_link_but_no_credential() {
        let diagnostics = Recorder::new();

        let status = run(
            &diagnostics,
            &variables(&[
                ("KESTREL_LINK", "http://127.0.0.1:1"),
                (
                    "KESTREL_INSTANCE",
                    "local-exec/kestrel-01999cf2-0000-7000-8000-000000000000",
                ),
            ]),
        )
        .await;

        assert_eq!(status, 1);
        assert!(diagnostics.everything_it_said().contains("no link to dial"));
    }

    #[test]
    fn the_bound_is_the_lease_it_was_handed_plus_a_margin() {
        assert_eq!(
            give_up_after(&variables(&[("KESTREL_LEASE", "120")])),
            Some(Duration::from_secs(120) + GIVE_UP_MARGIN)
        );
    }

    #[test]
    fn the_interrupt_deadline_is_thirty_seconds_unless_configuration_says_otherwise() {
        assert_eq!(interrupt_deadline(&variables(&[])), INTERRUPT_DEADLINE);
        assert_eq!(
            interrupt_deadline(&variables(&[("KESTREL_INTERRUPT_DEADLINE", "1")])),
            Duration::from_secs(1)
        );
        assert_eq!(
            interrupt_deadline(&variables(&[(
                "KESTREL_INTERRUPT_DEADLINE",
                "not a number"
            )])),
            INTERRUPT_DEADLINE
        );
    }

    #[test]
    fn a_supervisor_handed_no_lease_reconnects_without_a_bound() {
        assert_eq!(give_up_after(&variables(&[])), None);
        assert_eq!(
            give_up_after(&variables(&[("KESTREL_LEASE", "not a number")])),
            None
        );
    }

    #[test]
    fn a_lease_that_leaves_no_room_for_the_margin_is_not_a_bound() {
        assert_eq!(
            give_up_after(&variables(&[("KESTREL_LEASE", &u64::MAX.to_string())])),
            None
        );
    }
}
