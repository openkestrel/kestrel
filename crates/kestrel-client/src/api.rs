use anyhow::{Result, anyhow};
use reqwest::{Client, RequestBuilder, Response, Url, header};
use serde::Serialize;
use serde_json::Value;

use crate::diagnostic::{self, Request};
use crate::exit::{Exit, Failed};

pub struct ControlPlane {
    client: Client,
    base: Url,
    operation: String,
}

impl ControlPlane {
    /// `operation` names the command a lost or unrecognised answer is reported against.
    pub fn at(base: Url, operation: &str) -> Self {
        Self {
            client: Client::new(),
            base,
            operation: operation.to_owned(),
        }
    }

    pub async fn get(&self, path: &[&str]) -> Result<Value> {
        self.get_where(path, &[]).await
    }

    pub async fn get_where(&self, path: &[&str], query: &[(&str, &str)]) -> Result<Value> {
        let mut url = self.url(path)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }

        self.answered(self.client.get(url), false).await
    }

    pub async fn get_response(&self, path: &[&str], query: &[(&str, &str)]) -> Result<Response> {
        let mut url = self.url(path)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }

        self.sent(self.client.get(url), false).await
    }

    pub async fn post(&self, path: &[&str], body: &impl Serialize) -> Result<Value> {
        self.answered(self.client.post(self.url(path)?).json(body), true)
            .await
    }

    pub async fn put(&self, path: &[&str], body: &impl Serialize) -> Result<Value> {
        self.answered(self.client.put(self.url(path)?).json(body), true)
            .await
    }

    pub async fn delete(&self, path: &[&str], body: &impl Serialize) -> Result<()> {
        self.sent(self.client.delete(self.url(path)?).json(body), true)
            .await?;
        Ok(())
    }

    async fn answered(&self, request: RequestBuilder, write: bool) -> Result<Value> {
        let response = self.sent(request, write).await?;
        let status = response.status();
        response
            .json()
            .await
            .map_err(|error| anyhow!(diagnostic::unreadable(status, self.request(write), &error)))
    }

    fn request(&self, write: bool) -> Request<'_> {
        Request {
            operation: &self.operation,
            write,
        }
    }

    async fn sent(&self, request: RequestBuilder, write: bool) -> Result<Response> {
        let response = request.send().await.map_err(|error| {
            anyhow!(diagnostic::unreachable(
                self.base.as_str().trim_end_matches('/'),
                self.request(write),
                !error.is_connect(),
            ))
        })?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let retry_after = retry_after(&response);
        let body = response.bytes().await.unwrap_or_default();
        Err(anyhow!(diagnostic::refused(
            status,
            retry_after,
            &body,
            self.request(write),
        )))
    }

    fn url(&self, path: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|()| {
                Failed::new(
                    Exit::Usage,
                    format!("{} cannot be a base for a path", self.base),
                )
            })?
            .pop_if_empty()
            .push("operator")
            .extend(path);

        Ok(url)
    }
}

pub fn retry_after(response: &Response) -> Option<i64> {
    response
        .headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}
