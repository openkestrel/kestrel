//! A browser tab's one event stream: soft state the tab rebuilds after any drop (ADR-0045).

use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::BoxError;
use futures_core::Stream;
use tokio::sync::Notify;
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

    pub fn ends(&self) -> bool {
        self.event == END
    }
}

pub type Events = Pin<Box<dyn Stream<Item = Result<Emitted, BoxError>> + Send>>;

pub const END: &str = "end";

pub struct Subscribed {
    pub id: String,
    pub generation: u64,
    pub events: Events,
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

/// Only the subscriptions the tab wants are kept, never a queue of changes to them, so a
/// connection that stops reading cannot make them pile up.
struct Reservation {
    held: HashMap<String, u64>,
    unstarted: HashMap<String, Subscribed>,
    connected: bool,
    wake: Arc<Notify>,
    generations: u64,
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
                    held: HashMap::new(),
                    unstarted: HashMap::new(),
                    connected: false,
                    wake: Arc::new(Notify::new()),
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
                .is_some_and(|reservation| !reservation.connected)
            {
                reservations.remove(&token);
            }
        });

        Ok(token)
    }

    /// The reservation lasts exactly as long as the connection: once it drops, the tab reserves
    /// again and re-subscribes from the cursors it saw.
    pub fn connect(&self, token: Token) -> Result<Connected, Refusal> {
        let mut reservations = self.hub.reservations.lock().unwrap();
        let reservation = reservations.get_mut(&token).ok_or(Refusal::Unknown)?;
        if reservation.connected {
            return Err(Refusal::AlreadyConnected);
        }
        reservation.connected = true;

        Ok(Connected {
            hub: self.hub.clone(),
            token,
            wake: reservation.wake.clone(),
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
        reservation.unstarted.insert(
            id.clone(),
            Subscribed {
                id,
                generation,
                events,
            },
        );
        reservation.wake.notify_one();

        Ok(())
    }

    pub fn unsubscribe(&self, token: Token, id: String) -> Result<(), Refusal> {
        let mut reservations = self.hub.reservations.lock().unwrap();
        let reservation = reservations.get_mut(&token).ok_or(Refusal::Unknown)?;
        if reservation.held.remove(&id).is_some() {
            reservation.unstarted.remove(&id);
            reservation.wake.notify_one();
        }

        Ok(())
    }
}

/// Dropping it forgets the reservation, however the connection ended.
pub struct Connected {
    hub: Arc<Hub>,
    token: Token,
    wake: Arc<Notify>,
}

/// What a connection should start, and every id it should hold with the generation it should
/// hold it at: anything else it runs was replaced or ended.
pub struct Wanted {
    pub unstarted: Vec<Subscribed>,
    pub held: HashMap<String, u64>,
}

impl Connected {
    pub async fn changed(&self) {
        self.wake.notified().await;
    }

    pub fn wanted(&self) -> Wanted {
        let mut reservations = self.hub.reservations.lock().unwrap();
        let Some(reservation) = reservations.get_mut(&self.token) else {
            return Wanted {
                unstarted: Vec::new(),
                held: HashMap::new(),
            };
        };

        Wanted {
            unstarted: std::mem::take(&mut reservation.unstarted)
                .into_values()
                .collect(),
            held: reservation.held.clone(),
        }
    }

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
