use tokio_util::sync::CancellationToken;

/// Holds one startup after publication so the live dispatcher can observe its supervisor's exit.
pub struct StartupCheckpoint(Pause);

#[derive(Clone)]
pub(super) struct Pause {
    reached: CancellationToken,
    resume: CancellationToken,
}

impl StartupCheckpoint {
    pub(super) fn new() -> Self {
        Self(Pause {
            reached: CancellationToken::new(),
            resume: CancellationToken::new(),
        })
    }

    pub(super) fn pause(&self) -> Pause {
        self.0.clone()
    }

    pub async fn reached(&self) {
        self.0.reached.cancelled().await;
    }
}

impl Drop for StartupCheckpoint {
    fn drop(&mut self) {
        self.0.resume.cancel();
    }
}

impl Pause {
    pub(super) async fn wait(self) {
        self.reached.cancel();
        self.resume.cancelled().await;
    }
}
