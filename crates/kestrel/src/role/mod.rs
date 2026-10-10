pub mod serve;
pub mod work;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::capability::Images;
use crate::store::Store;
use crate::timer::Wake;
use std::time::Duration;
use url::Url;

pub struct AllInOne {
    store: Store,
    listening: serve::Listening,
    wake: Wake,
}

/// One process is the only place ingest can wake the sweeps that consume what it recorded.
pub async fn bind(
    store: Store,
    listen: serve::Listen,
    client: Url,
    follow_lease: Duration,
) -> Result<AllInOne> {
    let wake = Wake::default();

    Ok(AllInOne {
        store: store.clone(),
        listening: serve::bind(store, listen, client, wake.clone(), follow_lease).await?,
        wake,
    })
}

impl AllInOne {
    pub fn bound(&self) -> serve::Listen {
        self.listening.bound()
    }

    pub async fn run(
        self,
        dispatch: Option<work::Dispatch>,
        shutdown: CancellationToken,
    ) -> Result<()> {
        let summaries = self.listening.live.summaries.clone();
        let listening = match &dispatch {
            Some(dispatch) => self.listening.inspecting(Images::of(&dispatch.driver)),
            None => self.listening,
        };
        let serve = tokio::spawn(stopping_the_others(shutdown.clone(), |shutdown| {
            serve::run(listening, shutdown)
        }));
        let work = tokio::spawn(stopping_the_others(shutdown.clone(), |shutdown| {
            work::run(self.store, dispatch, self.wake, summaries, shutdown)
        }));

        let (serve, work) = tokio::join!(serve, work);
        serve??;
        work??;
        Ok(())
    }
}

/// The drop guard fires however `role` returns, so no sibling outlives it.
async fn stopping_the_others<Role, Running>(
    shutdown: CancellationToken,
    role: Role,
) -> anyhow::Result<()>
where
    Role: FnOnce(CancellationToken) -> Running,
    Running: Future<Output = anyhow::Result<()>>,
{
    let _stop_the_others = shutdown.clone().drop_guard();
    role(shutdown).await
}
