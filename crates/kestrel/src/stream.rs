//! A browser tab's one event stream: soft state the tab rebuilds after any drop (ADR-0045).

use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::BoxError;
use futures_core::Stream;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::log::Cursor;

pub const MOST_RESERVATIONS: usize = 256;
pub const MOST_SUBSCRIPTIONS: usize = 32;
const LONGEST_SUBSCRIPTION_ID: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token(pub Uuid);

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Token {
    type Err = uuid::Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self(text.parse()?))
    }
}

/// One event of a per-resource stream, before it is framed for its connection.
pub struct Emitted {
    pub event: &'static str,
    pub cursor: Option<Cursor>,
    pub data: serde_json::Value,
}

impl Emitted {
    pub fn new(
        event: &'static str,
        cursor: Option<Cursor>,
        data: &impl serde::Serialize,
    ) -> serde_json::Result<Self> {
        Ok(Self {
            event,
            cursor,
            data: serde_json::to_value(data)?,
        })
    }
}

pub type Events = Pin<Box<dyn Stream<Item = Result<Emitted, BoxError>> + Send>>;

pub const END: &str = "end";

impl Emitted {
    pub fn ends(&self) -> bool {
        self.event == END
    }
}

pub struct Subscribed {
    pub id: String,
    pub generation: u64,
    pub events: Events,
}

pub enum Change {
    Subscribe(Subscribed),
    Unsubscribe(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Unknown,
    AlreadyConnected,
    TooManyReservations,
    TooManySubscriptions,
    IdOutOfBounds,
}

pub fn bounded_id(id: &str) -> Result<(), Refusal> {
    if id.is_empty() || id.len() > LONGEST_SUBSCRIPTION_ID {
        return Err(Refusal::IdOutOfBounds);
    }

    Ok(())
}

#[derive(Clone)]
pub struct Streams {
    hub: Arc<Hub>,
}

struct Hub {
    unconnected_for: Duration,
    reservations: Mutex<HashMap<Token, Reservation>>,
}

struct Reservation {
    state: State,
    held: HashMap<String, u64>,
    generations: u64,
}

/// Subscriptions made before the connection opens wait in place, so one id held over and over
/// queues nothing.
enum State {
    Waiting(HashMap<String, Subscribed>),
    Connected(mpsc::UnboundedSender<Change>),
}

impl Streams {
    pub fn new(unconnected_for: Duration) -> Self {
        Self {
            hub: Arc::new(Hub {
                unconnected_for,
                reservations: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn reserve(&self) -> Result<Token, Refusal> {
        let token = Token(Uuid::now_v7());
        {
            let mut reservations = self.hub.reservations.lock().unwrap();
            if reservations.len() >= MOST_RESERVATIONS {
                return Err(Refusal::TooManyReservations);
            }
            reservations.insert(
                token,
                Reservation {
                    state: State::Waiting(HashMap::new()),
                    held: HashMap::new(),
                    generations: 0,
                },
            );
        }

        let hub = self.hub.clone();
        tokio::spawn(async move {
            tokio::time::sleep(hub.unconnected_for).await;
            let mut reservations = hub.reservations.lock().unwrap();
            if reservations
                .get(&token)
                .is_some_and(|reservation| matches!(reservation.state, State::Waiting(_)))
            {
                reservations.remove(&token);
            }
        });

        Ok(token)
    }

    /// The reservation lasts exactly as long as the connection: once it drops, the tab reserves
    /// again and re-subscribes from the cursors it saw.
    pub fn connect(&self, token: Token) -> Result<Connection, Refusal> {
        let mut reservations = self.hub.reservations.lock().unwrap();
        let reservation = reservations.get_mut(&token).ok_or(Refusal::Unknown)?;
        let State::Waiting(waiting) = &mut reservation.state else {
            return Err(Refusal::AlreadyConnected);
        };
        let waiting = std::mem::take(waiting).into_values().collect();
        let (sender, changes) = mpsc::unbounded_channel();
        reservation.state = State::Connected(sender);

        Ok(Connection {
            connected: Connected {
                hub: self.hub.clone(),
                token,
            },
            waiting,
            changes,
        })
    }

    pub fn known(&self, token: Token) -> bool {
        self.hub.reservations.lock().unwrap().contains_key(&token)
    }

    pub fn subscribe(&self, token: Token, id: String, events: Events) -> Result<(), Refusal> {
        bounded_id(&id)?;
        let mut reservations = self.hub.reservations.lock().unwrap();
        let reservation = reservations.get_mut(&token).ok_or(Refusal::Unknown)?;
        if !reservation.held.contains_key(&id) && reservation.held.len() >= MOST_SUBSCRIPTIONS {
            return Err(Refusal::TooManySubscriptions);
        }
        reservation.generations += 1;
        let generation = reservation.generations;
        reservation.held.insert(id.clone(), generation);
        let subscribed = Subscribed {
            id,
            generation,
            events,
        };
        match &mut reservation.state {
            State::Waiting(waiting) => {
                waiting.insert(subscribed.id.clone(), subscribed);
            }
            State::Connected(changes) => {
                let _ = changes.send(Change::Subscribe(subscribed));
            }
        }

        Ok(())
    }

    pub fn unsubscribe(&self, token: Token, id: String) -> Result<(), Refusal> {
        let mut reservations = self.hub.reservations.lock().unwrap();
        let reservation = reservations.get_mut(&token).ok_or(Refusal::Unknown)?;
        if reservation.held.remove(&id).is_some() {
            match &mut reservation.state {
                State::Waiting(waiting) => {
                    waiting.remove(&id);
                }
                State::Connected(changes) => {
                    let _ = changes.send(Change::Unsubscribe(id));
                }
            }
        }

        Ok(())
    }
}

pub struct Connection {
    pub connected: Connected,
    pub waiting: Vec<Subscribed>,
    pub changes: mpsc::UnboundedReceiver<Change>,
}

/// Dropping it forgets the reservation, however the connection ended.
pub struct Connected {
    hub: Arc<Hub>,
    token: Token,
}

impl Connected {
    /// A replacement may already hold the id, so only the generation that ended lets it go.
    pub fn ended(&self, subscribed: &str, generation: u64) {
        let mut reservations = self.hub.reservations.lock().unwrap();
        if let Some(reservation) = reservations.get_mut(&self.token)
            && reservation.held.get(subscribed) == Some(&generation)
        {
            reservation.held.remove(subscribed);
        }
    }
}

impl Drop for Connected {
    fn drop(&mut self) {
        self.hub.reservations.lock().unwrap().remove(&self.token);
    }
}
