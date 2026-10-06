//! Stands in for the GitHub API: serves scripted responses and records what was sent to it,
//! so a test never needs a live GitHub account to exercise polling or an outbound comment.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub url: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ScriptedResponse {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

impl ScriptedResponse {
    pub fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    pub fn answering(status: u16) -> Self {
        Self {
            status,
            body: String::new(),
            headers: Vec::new(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

/// What a GitHub App's hook is sent: the `X-GitHub-Event` name and the webhook payload.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub event: String,
    pub payload: serde_json::Value,
}

pub const REPOSITORY: &str = "jtmthf/kestrel";

pub fn labelled(issue: i64, label: &str) -> Delivery {
    issue_event(issue, "labeled", label)
}

pub fn unlabelled(issue: i64, label: &str) -> Delivery {
    issue_event(issue, "unlabeled", label)
}

pub fn issue_event(issue: i64, action: &str, label: &str) -> Delivery {
    Delivery {
        event: "issues".to_owned(),
        payload: serde_json::json!({
            "action": action,
            "label": { "name": label },
            "issue": an_issue(issue),
            "sender": { "login": "jtmthf" },
        }),
    }
}

fn an_issue(number: i64) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": format!("an issue numbered {number}"),
        "html_url": format!("https://github.com/{REPOSITORY}/issues/{number}"),
        "labels": [],
    })
}

pub fn assigned(issue: i64, assignee: &str, actor: &str) -> Delivery {
    let mut delivery = issue_event(issue, "assigned", "");
    delivery.payload["sender"]["login"] = actor.into();
    delivery.payload["assignee"] = serde_json::json!({ "login": assignee });
    delivery
        .payload
        .as_object_mut()
        .expect("a payload")
        .remove("label");
    delivery
}

pub fn issue(number: i64, labels: &[&str]) -> ScriptedResponse {
    ScriptedResponse::ok(
        serde_json::json!({
            "number": number,
            "state": "open",
            "title": format!("an issue numbered {number}"),
            "html_url": format!("https://github.com/{REPOSITORY}/issues/{number}"),
            "assignees": [{ "login": "kestrel" }],
            "labels": labels
                .iter()
                .map(|name| serde_json::json!({ "name": name }))
                .collect::<Vec<_>>(),
        })
        .to_string(),
    )
}

/// Labelled with `label` on an issue that already carries `carries` besides it.
pub fn labelled_carrying(issue: i64, label: &str, carries: &[&str]) -> Delivery {
    let mut delivery = labelled(issue, label);
    delivery.payload["issue"]["labels"] = std::iter::once(label)
        .chain(carries.iter().copied())
        .map(|name| serde_json::json!({ "name": name }))
        .collect();
    delivery
}

/// One comment as GitHub answers a newly posted one, or a read of one.
pub fn comment(id: i64, body: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "html_url": format!("https://github.com/{REPOSITORY}/issues/43#issuecomment-{id}"),
        "body": body,
    })
}

pub fn issue_comment(id: i64, issue: i64, actor: &str, body: &str) -> Delivery {
    Delivery {
        event: "issue_comment".to_owned(),
        payload: serde_json::json!({
            "action": "created",
            "issue": an_issue(issue),
            "comment": {
                "id": id,
                "html_url": format!("https://github.com/{REPOSITORY}/issues/{issue}#issuecomment-{id}"),
                "issue_url": format!("https://api.github.com/repos/{REPOSITORY}/issues/{issue}"),
                "body": body,
                "user": { "login": actor },
            },
            "sender": { "login": actor },
        }),
    }
}

pub fn created(id: i64, body: &str) -> ScriptedResponse {
    ScriptedResponse {
        status: 201,
        body: comment(id, body).to_string(),
        headers: Vec::new(),
    }
}

pub fn page(events: &[serde_json::Value]) -> ScriptedResponse {
    ScriptedResponse::ok(serde_json::Value::Array(events.to_vec()).to_string())
}

/// The installation token the stub hands out when a mint is not scripted for a specific test:
/// distinctive enough that a test can assert it is nowhere it should not be.
pub const INSTALLATION_TOKEN: &str = "ghs_kestrel_should_never_say_this_out_loud";

/// What `POST /app/installations/{id}/access_tokens` answers, good for `good_for` from the
/// moment it is minted: a test scripts a short lifetime to make a cached token go stale on
/// demand, rather than waiting out a real installation token's real hour.
pub fn minted_token(token: &str, good_for: jiff::SignedDuration) -> ScriptedResponse {
    ScriptedResponse::ok(
        serde_json::json!({
            "token": token,
            "expires_at": (jiff::Timestamp::now() + good_for).to_string(),
        })
        .to_string(),
    )
}

/// What `POST /app/installations/{id}/access_tokens` answers by default, good for an hour so a
/// test exercising something else never has to think about it.
fn minted_installation_token() -> ScriptedResponse {
    minted_token(INSTALLATION_TOKEN, jiff::SignedDuration::from_hours(1))
}

/// What `GET /app` answers by default: a GitHub App's own identity, learned once at
/// registration.
fn default_app_record() -> ScriptedResponse {
    ScriptedResponse::ok(serde_json::json!({ "slug": "kestrel" }).to_string())
}

/// An exhausted quota, with a reset that has already passed so a test is not held at it.
pub fn rate_limited() -> ScriptedResponse {
    ScriptedResponse::answering(403)
        .with_header("x-ratelimit-remaining", "0")
        .with_header(
            "x-ratelimit-reset",
            &jiff::Timestamp::now().as_second().to_string(),
        )
}

/// One entry in the App's Delivery log. It is made when the stub is next asked for the log, so a
/// test may deliver before kestrel has registered the Integration that reads it.
struct Logged {
    id: i64,
    guid: String,
    delivered_at: Option<jiff::Timestamp>,
    installation_id: Option<i64>,
    delivery: Delivery,
}

/// A queue of responses for one endpoint, so a sweep polling for events cannot take a
/// response scripted for an outbound comment.
struct Endpoint {
    method: String,
    path: String,
    responses: VecDeque<ScriptedResponse>,
}

pub struct GithubStub {
    port: u16,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    responses: Arc<Mutex<VecDeque<ScriptedResponse>>>,
    endpoints: Arc<Mutex<Vec<Endpoint>>>,
    log: Arc<Mutex<Vec<Logged>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GithubStub {
    pub fn start() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("the stub should bind a port");
        let port = server
            .server_addr()
            .to_ip()
            .expect("bound over IP, not a unix socket")
            .port();

        let requests = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(VecDeque::new()));
        let endpoints = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let thread = {
            let requests = Arc::clone(&requests);
            let responses = Arc::clone(&responses);
            let endpoints = Arc::clone(&endpoints);
            let log = Arc::clone(&log);
            let stop = Arc::clone(&stop);

            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let request = match server.recv_timeout(Duration::from_millis(50)) {
                        Ok(Some(request)) => request,
                        Ok(None) => continue,
                        Err(_) => break,
                    };

                    respond(request, &requests, &responses, &endpoints, &log);
                }
            })
        };

        Self {
            port,
            requests,
            responses,
            endpoints,
            log,
            stop,
            thread: Some(thread),
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn script(&self, response: ScriptedResponse) {
        self.responses
            .lock()
            .expect("the response queue should not be poisoned")
            .push_back(response);
    }

    /// Scripts a response for one endpoint, matched by method and by a fragment of the path.
    pub fn script_answer(&self, method: &str, path: &str, response: ScriptedResponse) {
        let mut endpoints = self
            .endpoints
            .lock()
            .expect("the endpoint queues should not be poisoned");

        match endpoints
            .iter_mut()
            .find(|endpoint| endpoint.method == method && endpoint.path == path)
        {
            Some(endpoint) => endpoint.responses.push_back(response),
            None => endpoints.push(Endpoint {
                method: method.to_owned(),
                path: path.to_owned(),
                responses: VecDeque::from([response]),
            }),
        }
    }

    /// Logs a Delivery for the harness's installation and answers its GUID, the
    /// `X-GitHub-Delivery` a webhook carrying it would name.
    pub fn deliver(&self, delivery: Delivery) -> String {
        self.log_delivery(delivery, None, Some(INSTALLATION))
    }

    /// As GitHub logs a Delivery made at `at`, rather than when it is next asked.
    pub fn deliver_at(&self, delivery: Delivery, at: jiff::Timestamp) -> String {
        self.log_delivery(delivery, Some(at), Some(INSTALLATION))
    }

    /// As GitHub logs a Delivery whose webhook already carried `guid`.
    pub fn deliver_as(&self, delivery: Delivery, guid: &str) {
        self.log_delivery(delivery, None, Some(INSTALLATION));
        let mut log = self
            .log
            .lock()
            .expect("the delivery log should not be poisoned");
        log.last_mut().expect("just logged").guid = guid.to_owned();
    }

    /// A Delivery for another installation of the same App.
    pub fn deliver_elsewhere(&self, delivery: Delivery, installation: i64) -> String {
        self.log_delivery(delivery, None, Some(installation))
    }

    fn log_delivery(
        &self,
        delivery: Delivery,
        at: Option<jiff::Timestamp>,
        installation_id: Option<i64>,
    ) -> String {
        let mut log = self
            .log
            .lock()
            .expect("the delivery log should not be poisoned");
        let id = i64::try_from(log.len()).expect("a small log") + 1;
        let guid = format!("00000000-0000-4000-8000-{id:012}");
        log.push(Logged {
            id,
            guid: guid.clone(),
            delivered_at: at,
            installation_id,
            delivery,
        });
        guid
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .expect("the request log should not be poisoned")
            .clone()
    }
}

fn respond(
    mut request: tiny_http::Request,
    requests: &Mutex<Vec<RecordedRequest>>,
    responses: &Mutex<VecDeque<ScriptedResponse>>,
    endpoints: &Mutex<Vec<Endpoint>>,
    log: &Mutex<Vec<Logged>>,
) {
    let headers = request
        .headers()
        .iter()
        .map(|header| {
            (
                header.field.as_str().as_str().to_lowercase(),
                header.value.as_str().to_owned(),
            )
        })
        .collect();
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);

    requests
        .lock()
        .expect("the request log should not be poisoned")
        .push(RecordedRequest {
            method: request.method().to_string(),
            url: request.url().to_owned(),
            body,
            headers,
        });

    let method = request.method().to_string();
    let url = request.url().to_owned();
    let host = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("host"))
        .map(|header| header.value.as_str().to_owned())
        .unwrap_or_default();
    let scripted = endpoints
        .lock()
        .expect("the endpoint queues should not be poisoned")
        .iter_mut()
        .filter(|endpoint| {
            method == endpoint.method
                && url.contains(&endpoint.path)
                && !endpoint.responses.is_empty()
        })
        .max_by_key(|endpoint| endpoint.path.len())
        .and_then(|endpoint| endpoint.responses.pop_front());
    let scripted = scripted.or_else(|| {
        if method == "POST" && url.contains("/access_tokens") {
            Some(minted_installation_token())
        } else if method == "GET" && url.ends_with("/app") {
            Some(default_app_record())
        } else {
            None
        }
    });
    let scripted = scripted.or_else(|| {
        if method != "GET" {
            return None;
        }
        let tail = url.split("/issues/").nth(1)?;
        if let Ok(number) = tail.parse::<i64>() {
            Some(issue(number, &[]))
        } else if tail.contains("/dependencies/blocked_by?") {
            Some(page(&[]))
        } else {
            None
        }
    });
    let scripted = scripted.or_else(|| {
        if method != "GET" {
            return None;
        }
        let mut log = log.lock().expect("the delivery log should not be poisoned");
        if url.starts_with("/app/hook/deliveries?") {
            Some(listed(&mut log, &url, &host))
        } else {
            let id: i64 = url.strip_prefix("/app/hook/deliveries/")?.parse().ok()?;
            log.iter()
                .find(|logged| logged.id == id)
                .map(|logged| ScriptedResponse::ok(delivered(logged).to_string()))
        }
    });
    let scripted = scripted.or_else(|| {
        responses
            .lock()
            .expect("the response queue should not be poisoned")
            .pop_front()
    });

    let scripted = scripted.unwrap_or_else(|| ScriptedResponse::answering(404));

    let mut response =
        tiny_http::Response::from_string(scripted.body).with_status_code(scripted.status);
    for (name, value) in &scripted.headers {
        let header = tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
            .expect("a header the stub was asked to send");
        response.add_header(header);
    }
    let _ = request.respond(response);
}

/// The installation the harness registers its GitHub Integrations against.
const INSTALLATION: i64 = 2;

/// Newest first, as GitHub lists it, a page at a time with the next named by cursor. What has not
/// been listed before is made now, in the order it was delivered.
fn listed(log: &mut [Logged], url: &str, host: &str) -> ScriptedResponse {
    let parameter = |name: &str| {
        url.split(['?', '&'])
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .and_then(|value| value.parse::<usize>().ok())
    };
    let per_page = parameter("per_page").unwrap_or(30);
    let cursor = parameter("cursor").unwrap_or(0);
    let mut at = jiff::Timestamp::now();
    for logged in log
        .iter_mut()
        .filter(|logged| logged.delivered_at.is_none())
    {
        at += jiff::SignedDuration::from_millis(1);
        logged.delivered_at = Some(at);
    }
    let mut newest_first: Vec<&Logged> = log.iter().collect();
    newest_first.sort_by_key(|logged| std::cmp::Reverse((logged.delivered_at, logged.id)));
    let more = newest_first.len() > cursor + per_page;
    let entries: Vec<_> = newest_first
        .into_iter()
        .skip(cursor)
        .take(per_page)
        .map(|logged| {
            serde_json::json!({
                "id": logged.id,
                "guid": logged.guid,
                "delivered_at": logged.delivered_at.expect("made above").to_string(),
                "redelivery": false,
                "status": "failed to connect to host",
                "status_code": 502,
                "event": logged.delivery.event,
                "action": logged.delivery.payload.get("action"),
                "installation_id": logged.installation_id,
                "repository_id": 1,
            })
        })
        .collect();
    let page = ScriptedResponse::ok(serde_json::Value::Array(entries).to_string());
    if more {
        page.with_header(
            "link",
            &format!(
                "<http://{host}/app/hook/deliveries?per_page={per_page}&cursor={}>; rel=\"next\"",
                cursor + per_page
            ),
        )
    } else {
        page
    }
}

fn delivered(logged: &Logged) -> serde_json::Value {
    serde_json::json!({
        "id": logged.id,
        "guid": logged.guid,
        "delivered_at": logged.delivered_at.map(|at| at.to_string()),
        "event": logged.delivery.event,
        "action": logged.delivery.payload.get("action"),
        "installation_id": logged.installation_id,
        "request": {
            "headers": {
                "X-GitHub-Delivery": logged.guid,
                "X-GitHub-Event": logged.delivery.event,
            },
            "payload": logged.delivery.payload,
        },
        "response": { "headers": null, "payload": "" },
    })
}

impl Drop for GithubStub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    use super::*;

    fn get(base_url: &str, path: &str) -> (u16, String) {
        let host_port = base_url.trim_start_matches("http://");
        let mut stream =
            TcpStream::connect(host_port).expect("the stub should accept a connection");
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .expect("the request should send");

        let mut raw = String::new();
        stream
            .read_to_string(&mut raw)
            .expect("the response should read");

        let status = raw
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .expect("a status line");
        let body = raw
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or("");

        (status, body.to_owned())
    }

    fn post(base_url: &str, path: &str, body: &str) -> u16 {
        let host_port = base_url.trim_start_matches("http://");
        let mut stream =
            TcpStream::connect(host_port).expect("the stub should accept a connection");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .expect("the request should send");

        let mut raw = String::new();
        stream
            .read_to_string(&mut raw)
            .expect("the response should read");

        raw.lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .expect("a status line")
    }

    #[test]
    fn it_serves_a_scripted_response_and_records_the_request() {
        let stub = GithubStub::start();
        stub.script(ScriptedResponse::ok("[]"));

        let (status, body) = get(&stub.base_url(), "/repos/acme/kestrel/issues/events");

        assert_eq!(status, 200);
        assert_eq!(body, "[]");

        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].url, "/repos/acme/kestrel/issues/events");
    }

    #[test]
    fn an_unscripted_request_gets_a_404_rather_than_hanging() {
        let stub = GithubStub::start();

        let (status, _) = get(&stub.base_url(), "/anything");

        assert_eq!(status, 404);
    }

    #[test]
    fn responses_are_served_in_the_order_they_were_scripted() {
        let stub = GithubStub::start();
        stub.script(ScriptedResponse::ok("first"));
        stub.script(ScriptedResponse::ok("second"));

        let (_, first) = get(&stub.base_url(), "/a");
        let (_, second) = get(&stub.base_url(), "/b");

        assert_eq!(first, "first");
        assert_eq!(second, "second");
    }

    #[test]
    fn it_records_the_body_of_an_outbound_post() {
        let stub = GithubStub::start();
        stub.script(ScriptedResponse::answering(201));

        let status = post(
            &stub.base_url(),
            "/repos/acme/kestrel/issues/1/comments",
            "{\"body\":\"done\"}",
        );

        assert_eq!(status, 201);

        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].body, "{\"body\":\"done\"}");
    }
}
