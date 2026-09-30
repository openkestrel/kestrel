use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tokio::sync::{OnceCell, mpsc, oneshot};
use tokio::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::declined::Declined;
use crate::store::Store;

const ANSWER_BEGUN_WITHIN: Duration = Duration::from_secs(10);
const SHARED_FOR: Duration = Duration::from_secs(5);
const SHARED_UP_TO: usize = 2 * 1024 * 1024;
/// Past what a listing of 5,000 entries or 1 MiB of escaped text can reach.
const BUFFERED_UP_TO: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "read", rename_all = "snake_case")]
pub enum Read {
    Files {
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    File {
        path: String,
        raw: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub request: Uuid,
    #[serde(flatten)]
    pub read: Read,
}

#[derive(Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
enum Answer {
    Listing(Listing),
    Text(Text),
    Refused { message: String },
    Missing { message: String },
}

#[derive(Serialize, Deserialize)]
struct Listing {
    path: String,
    entries: Vec<Entry>,
    total: u64,
    truncated: bool,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    name: String,
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Text {
    path: String,
    text: String,
}

pub enum AnswerBody {
    Json(Bytes),
    Raw(Body),
}

impl IntoResponse for AnswerBody {
    fn into_response(self) -> Response {
        match self {
            AnswerBody::Json(json) => {
                ([(header::CONTENT_TYPE, "application/json")], json).into_response()
            }
            AnswerBody::Raw(body) => {
                ([(header::CONTENT_TYPE, "application/octet-stream")], body).into_response()
            }
        }
    }
}

#[derive(Debug)]
pub struct NotAnswering;

impl fmt::Display for NotAnswering {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("the Instance didn't answer")
    }
}

impl std::error::Error for NotAnswering {}

#[derive(Clone)]
enum Outcome {
    Json(Bytes),
    Refused(String),
    Missing(String),
    /// Handed to the one read that asked; anyone sharing it asks again for the bytes alone.
    Raw,
    NotAnswering,
}

struct Arriving {
    body: Body,
    json: bool,
    /// Dropped once the answer has been read, which is when the supervisor's POST is answered.
    taken: oneshot::Sender<()>,
}

type Shared = Arc<OnceCell<(Outcome, Instant)>>;

#[derive(Default)]
struct Live {
    streams: HashMap<String, Vec<(u64, mpsc::UnboundedSender<Request>)>>,
    opened: u64,
    awaiting: HashMap<Uuid, (String, oneshot::Sender<Arriving>)>,
    shared: HashMap<(String, Read), Shared>,
}

/// Where the operator's reads meet the supervisor's answers: in the serve role's memory, which
/// holds every Instance's stream.
#[derive(Clone, Default)]
pub struct Reads(Arc<Mutex<Live>>);

pub struct Stream {
    reads: Reads,
    instance: String,
    opened: u64,
}

impl Drop for Stream {
    fn drop(&mut self) {
        let mut live = self.reads.0.lock().unwrap();
        if let Some(streams) = live.streams.get_mut(&self.instance) {
            streams.retain(|(opened, _)| *opened != self.opened);
            if streams.is_empty() {
                live.streams.remove(&self.instance);
            }
        }
    }
}

struct Awaited {
    reads: Reads,
    request: Uuid,
}

impl Drop for Awaited {
    fn drop(&mut self) {
        self.reads.0.lock().unwrap().awaiting.remove(&self.request);
    }
}

impl Reads {
    pub fn connected(&self, instance: &str) -> (mpsc::UnboundedReceiver<Request>, Stream) {
        let (asking, asked) = mpsc::unbounded_channel();
        let mut live = self.0.lock().unwrap();
        live.opened += 1;
        let opened = live.opened;
        live.streams
            .entry(instance.to_owned())
            .or_default()
            .push((opened, asking));

        (
            asked,
            Stream {
                reads: self.clone(),
                instance: instance.to_owned(),
                opened,
            },
        )
    }

    pub fn answer(
        &self,
        instance: &str,
        request: Uuid,
        body: Body,
        json: bool,
    ) -> Option<oneshot::Receiver<()>> {
        let mut live = self.0.lock().unwrap();
        if live.awaiting.get(&request)?.0 != instance {
            return None;
        }
        let (_, arriving) = live.awaiting.remove(&request)?;
        let (taken, answered) = oneshot::channel();
        arriving.send(Arriving { body, json, taken }).ok()?;

        Some(answered)
    }

    async fn read(&self, instance: &str, read: Read) -> Result<AnswerBody, anyhow::Error> {
        if matches!(read, Read::File { raw: true, .. }) {
            let (outcome, raw) = self.asked(instance, read.clone()).await;
            return answered(outcome, raw);
        }

        let key = (instance.to_owned(), read.clone());
        let shared = {
            let mut live = self.0.lock().unwrap();
            live.shared
                .retain(|_, shared| shared.get().is_none_or(|(_, at)| at.elapsed() < SHARED_FOR));
            Arc::clone(live.shared.entry(key.clone()).or_default())
        };
        let mut raw = None;
        let (outcome, _) = shared
            .get_or_init(|| async {
                let (outcome, body) = self.asked(instance, read.clone()).await;
                raw = body;
                (outcome, Instant::now())
            })
            .await
            .clone();
        if !matches!(&outcome, Outcome::Json(json) if json.len() <= SHARED_UP_TO) {
            let mut live = self.0.lock().unwrap();
            if live
                .shared
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, &shared))
            {
                live.shared.remove(&key);
            }
        }

        match (outcome, raw, read) {
            (Outcome::Raw, None, Read::File { path, .. }) => {
                let (outcome, raw) = self.asked(instance, Read::File { path, raw: true }).await;
                answered(outcome, raw)
            }
            (outcome, raw, _) => answered(outcome, raw),
        }
    }

    async fn asked(&self, instance: &str, read: Read) -> (Outcome, Option<Body>) {
        let request = Uuid::now_v7();
        let (arriving, arrived) = oneshot::channel();
        {
            let mut live = self.0.lock().unwrap();
            let Some(streams) = live.streams.get(instance) else {
                return (Outcome::NotAnswering, None);
            };
            for (_, stream) in streams {
                let _ = stream.send(Request {
                    request,
                    read: read.clone(),
                });
            }
            live.awaiting
                .insert(request, (instance.to_owned(), arriving));
        }
        let _awaited = Awaited {
            reads: self.clone(),
            request,
        };
        info!(instance, %request, ?read, "a read went down the link");

        let Ok(Ok(arrived)) = tokio::time::timeout(ANSWER_BEGUN_WITHIN, arrived).await else {
            return (Outcome::NotAnswering, None);
        };
        if !arrived.json {
            let taken = arrived.taken;
            let stream = async_stream::stream! {
                let _taken = taken;
                for await chunk in arrived.body.into_data_stream() {
                    yield chunk;
                }
            };
            return (Outcome::Raw, Some(Body::from_stream(stream)));
        }

        let outcome = match axum::body::to_bytes(arrived.body, BUFFERED_UP_TO).await {
            Ok(json) => match serde_json::from_slice::<Answer>(&json) {
                Ok(Answer::Listing(listing)) => serialized(&listing),
                Ok(Answer::Text(text)) => serialized(&text),
                Ok(Answer::Refused { message }) => Outcome::Refused(message),
                Ok(Answer::Missing { message }) => Outcome::Missing(message),
                Err(_) => Outcome::NotAnswering,
            },
            Err(_) => Outcome::NotAnswering,
        };
        drop(arrived.taken);

        (outcome, None)
    }
}

fn serialized(answer: &impl Serialize) -> Outcome {
    serde_json::to_vec(answer).map_or(Outcome::NotAnswering, |json| Outcome::Json(json.into()))
}

fn answered(outcome: Outcome, raw: Option<Body>) -> Result<AnswerBody, anyhow::Error> {
    match (outcome, raw) {
        (Outcome::Json(json), _) => Ok(AnswerBody::Json(json)),
        (Outcome::Raw, Some(body)) => Ok(AnswerBody::Raw(body)),
        (Outcome::Refused(why), _) => Err(Declined::Unacceptable(why).into()),
        (Outcome::Missing(why), _) => Err(Declined::Missing(why).into()),
        (Outcome::Raw | Outcome::NotAnswering, _) => Err(NotAnswering.into()),
    }
}

pub async fn read(
    store: &Store,
    reads: &Reads,
    organization: &str,
    reference: &str,
    read: Read,
) -> anyhow::Result<AnswerBody> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;
    let workspace = tx.workspaces().resolved(&organization, reference).await?;
    let Some(instance) = tx.workspaces().instance(workspace.id).await? else {
        return Err(Declined::Unacceptable(format!(
            "the Workspace {} has no Instance to read; its work is on the branch {}",
            workspace.name, workspace.checkout.branch
        ))
        .into());
    };
    drop(tx);

    reads.read(&instance, read).await
}
