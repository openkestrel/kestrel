/// The serve role's live per-Instance state: the work each Instance reports and the reads the
/// operator issues at it. Created once per role run, then cloned into both routers, which share
/// the one value.
#[derive(Clone, Default)]
pub struct Live {
    pub summaries: crate::live_work::Summaries,
    pub reads: crate::live_read::Reads,
}
