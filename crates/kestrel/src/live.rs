use crate::live_work::{Repository, Summary};
use crate::store::workspace::Linked;
use crate::timer::Clock;

/// The serve role's live per-Instance state: the work each Instance reports and the reads the
/// operator issues at it. Created once per role run, then cloned into both routers, which share
/// the one value.
#[derive(Clone, Default)]
pub struct Live {
    pub summaries: crate::live_work::Summaries,
    pub unrecorded: crate::live_work::Unrecorded,
    pub reads: crate::live_read::Reads,
    clock: Clock,
}

impl Live {
    pub fn on(clock: Clock) -> Self {
        Self {
            summaries: crate::live_work::Summaries::on(clock.clone()),
            unrecorded: crate::live_work::Unrecorded::default(),
            reads: crate::live_read::Reads::on(clock.clone()),
            clock,
        }
    }

    pub fn report_work(&self, reporter: Linked, repositories: Vec<Repository>) {
        let summary = Summary {
            repositories,
            reported_at: self.clock.now(),
        };
        self.summaries.report(&reporter.instance, summary.clone());
        self.unrecorded.report(reporter, summary);
    }
}
