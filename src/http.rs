use anyhow::{Result, anyhow, ensure};
use serde_json::Value;
use std::time::Duration;
pub type Response = ureq::http::Response<ureq::Body>;
#[derive(Clone)]
pub struct Http {
    pub agent: ureq::Agent,
}
impl Http {
    pub fn new(timeout: Duration) -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .max_redirects(0)
                .build()
                .into(),
        }
    }
    pub fn get(&self, url: &str, auth: Option<&str>) -> Result<Response> {
        let mut request = self.agent.get(url);
        if let Some(auth) = auth {
            request = request.header("Authorization", auth);
        }
        request
            .call()
            .map_err(|_| anyhow!("HTTP request failed or timed out"))
    }
    pub fn post(&self, url: &str, auth: Option<&str>, value: &Value) -> Result<Response> {
        let mut request = self.agent.post(url);
        if let Some(auth) = auth {
            request = request.header("Authorization", auth);
        }
        request
            .send_json(value)
            .map_err(|_| anyhow!("HTTP request failed or timed out"))
    }
    pub fn bytes(
        &self,
        url: &str,
        auth: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<Response> {
        self.agent
            .post(url)
            .header("Authorization", auth)
            .header("Content-Type", content_type)
            .send(bytes)
            .map_err(|_| anyhow!("HTTP upload failed or timed out"))
    }
}
pub fn body(mut response: Response, limit: u64) -> Result<Vec<u8>> {
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|_| anyhow!("HTTP response exceeded size limit or could not be read"))
}
pub fn json(response: Response) -> Result<Value> {
    let status = response.status().as_u16();
    ensure!(
        (200..300).contains(&status),
        "HTTP response status {status}"
    );
    serde_json::from_slice(&body(response, 1024 * 1024)?)
        .map_err(|_| anyhow!("Invalid JSON response"))
}
