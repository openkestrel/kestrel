//! Stands in for Anthropic's and OpenAI's model-metadata reads, so a saved key is checked without
//! a real account and a test can see exactly what was sent.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Asked {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl Asked {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone)]
pub enum Answer {
    Status(u16, String),
    /// Withheld until `release`, so a test can act while the check is in flight.
    Held(u16, String),
    /// Never answered, so the control plane's deadline decides.
    Silent,
}

pub fn models() -> Answer {
    Answer::Status(
        200,
        r#"{"data":[{"id":"a-model"}],"has_more":false}"#.to_owned(),
    )
}

pub fn rejected() -> Answer {
    Answer::Status(
        401,
        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#
            .to_owned(),
    )
}

pub fn forbidden() -> Answer {
    Answer::Status(
        403,
        r#"{"type":"error","error":{"type":"permission_error","message":"not allowed"}}"#
            .to_owned(),
    )
}

pub fn overloaded() -> Answer {
    Answer::Status(
        529,
        r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#.to_owned(),
    )
}

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    opened: Condvar,
}

pub struct ProviderStub {
    port: u16,
    asked: Arc<Mutex<Vec<Asked>>>,
    answers: Arc<Mutex<VecDeque<Answer>>>,
    gate: Arc<Gate>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ProviderStub {
    pub fn start() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("the stub should bind a port");
        let port = server
            .server_addr()
            .to_ip()
            .expect("bound over IP, not a unix socket")
            .port();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let answers = Arc::new(Mutex::new(VecDeque::new()));
        let gate = Arc::new(Gate::default());
        let stop = Arc::new(AtomicBool::new(false));

        let thread = {
            let asked = Arc::clone(&asked);
            let answers = Arc::clone(&answers);
            let gate = Arc::clone(&gate);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let request = match server.recv_timeout(Duration::from_millis(50)) {
                        Ok(Some(request)) => request,
                        Ok(None) => continue,
                        Err(_) => break,
                    };
                    asked
                        .lock()
                        .expect("the request log should not be poisoned")
                        .push(Asked {
                            method: request.method().to_string(),
                            url: request.url().to_owned(),
                            headers: request
                                .headers()
                                .iter()
                                .map(|header| {
                                    (
                                        header.field.as_str().as_str().to_lowercase(),
                                        header.value.as_str().to_owned(),
                                    )
                                })
                                .collect(),
                        });
                    let answer = answers
                        .lock()
                        .expect("the answers should not be poisoned")
                        .pop_front()
                        .unwrap_or_else(models);
                    let gate = Arc::clone(&gate);
                    std::thread::spawn(move || answer_with(request, answer, &gate));
                }
            })
        };

        Self {
            port,
            asked,
            answers,
            gate,
            stop,
            thread: Some(thread),
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Answers the next request; any request with nothing scripted lists models.
    pub fn answer(&self, answer: Answer) {
        self.answers
            .lock()
            .expect("the answers should not be poisoned")
            .push_back(answer);
    }

    pub fn release(&self) {
        *self
            .gate
            .open
            .lock()
            .expect("the gate should not be poisoned") = true;
        self.gate.opened.notify_all();
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.asked
            .lock()
            .expect("the request log should not be poisoned")
            .clone()
    }

    pub async fn until_asked(&self, times: usize) {
        for _ in 0..200 {
            if self.asked().len() >= times {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "the provider was asked {} times, not {times}",
            self.asked().len()
        );
    }
}

fn answer_with(request: tiny_http::Request, answer: Answer, gate: &Gate) {
    let (status, body) = match answer {
        Answer::Status(status, body) => (status, body),
        Answer::Held(status, body) => {
            let mut open = gate.open.lock().expect("the gate should not be poisoned");
            while !*open {
                open = gate
                    .opened
                    .wait(open)
                    .expect("the gate should not be poisoned");
            }
            (status, body)
        }
        Answer::Silent => {
            std::thread::sleep(Duration::from_secs(5));
            return;
        }
    };
    let response = tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(
            "content-type: application/json"
                .parse::<tiny_http::Header>()
                .expect("a header"),
        );
    let _ = request.respond(response);
}

impl Drop for ProviderStub {
    fn drop(&mut self) {
        self.release();
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
