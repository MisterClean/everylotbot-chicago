use crate::{
    config::Config,
    domain::Post,
    http::{self, Http},
};
use anyhow::{Result, ensure};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha1::Sha1;
use uuid::Uuid;
fn encode(value: &str) -> String {
    let mut result = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            result.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(result, "%{b:02X}");
        }
    }
    result
}
pub fn authorization(config: &Config, url: &str, nonce: &str, timestamp: &str) -> Result<String> {
    let mut params = vec![
        (
            "oauth_consumer_key",
            config.required("TWITTER_CONSUMER_KEY")?.to_owned(),
        ),
        ("oauth_nonce", nonce.to_owned()),
        ("oauth_signature_method", "HMAC-SHA1".to_owned()),
        ("oauth_timestamp", timestamp.to_owned()),
        (
            "oauth_token",
            config.required("TWITTER_ACCESS_TOKEN")?.to_owned(),
        ),
        ("oauth_version", "1.0".to_owned()),
    ];
    params.sort_by_key(|(key, _)| *key);
    let parameters = params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let base = format!("POST&{}&{}", encode(url), encode(&parameters));
    let signing = format!(
        "{}&{}",
        encode(config.required("TWITTER_CONSUMER_SECRET")?),
        encode(config.required("TWITTER_ACCESS_TOKEN_SECRET")?)
    );
    let mut mac = Hmac::<Sha1>::new_from_slice(signing.as_bytes())?;
    mac.update(base.as_bytes());
    params.push((
        "oauth_signature",
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()),
    ));
    params.sort_by_key(|(key, _)| *key);
    Ok(format!(
        "OAuth {}",
        params
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
fn auth(config: &Config, url: &str) -> Result<String> {
    authorization(
        config,
        url,
        &Uuid::new_v4().simple().to_string(),
        &chrono::Utc::now().timestamp().to_string(),
    )
}
pub fn upload(config: &Config, http: &Http, image: &[u8]) -> Result<String> {
    let url = "https://upload.twitter.com/1.1/media/upload.json";
    let boundary = format!("everylot-{}", Uuid::new_v4().simple());
    let mut multipart=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"media\"; filename=\"image.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n").into_bytes();
    multipart.extend_from_slice(image);
    multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let result = http::json(http.bytes(
        url,
        &auth(config, url)?,
        &format!("multipart/form-data; boundary={boundary}"),
        &multipart,
    )?)?;
    Ok(result["media_id_string"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Twitter upload response missing media ID"))?
        .to_owned())
}
pub fn create(config: &Config, http: &Http, post: &Post, media: &str) -> Result<String> {
    let url = "https://api.x.com/2/tweets";
    let result = http::json(http.post(
        url,
        Some(&auth(config, url)?),
        &json!({"text":post.text,"media":{"media_ids":[media]}}),
    )?)?;
    let id = result["data"]["id"].as_str().unwrap_or_default();
    ensure!(!id.is_empty(), "Twitter create response missing ID");
    Ok(id.to_owned())
}
