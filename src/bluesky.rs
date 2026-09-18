use crate::{
    config::Config,
    db::now,
    domain::Post,
    http::{self, Http},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

const ALPHABET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";
static LAST_MICROS: AtomicU64 = AtomicU64::new(0);
pub fn valid_key(key: &str) -> bool {
    key.len() == 13
        && b"234567abcdefghij".contains(&key.as_bytes()[0])
        && key.bytes().all(|b| ALPHABET.contains(&b))
}
pub fn new_key() -> Result<String> {
    let micros = u64::try_from(chrono::Utc::now().timestamp_micros())?;
    let last = LAST_MICROS
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| {
            Some(micros.max(last + 1))
        })
        .map_err(|_| anyhow!("Cannot generate record timestamp"))?;
    let timestamp = micros.max(last + 1);
    ensure!(timestamp < (1 << 53), "Record timestamp out of range");
    let random = Uuid::new_v4();
    let mut value = (timestamp << 10)
        | (u16::from_be_bytes([random.as_bytes()[0], random.as_bytes()[1]]) as u64 & 1023);
    let mut bytes = [b'2'; 13];
    for c in bytes.iter_mut().rev() {
        *c = ALPHABET[(value & 31) as usize];
        value >>= 5;
    }
    Ok(String::from_utf8(bytes.to_vec())?)
}

pub fn persist(path: &Path, session: &Value) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(parent)
            .context("Cannot create session directory")?;
    }
    let temp = parent.join(format!(".bluesky-session-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .context("Cannot create private session file")?;
        serde_json::to_writer(&mut file, session)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temp, path).context("Cannot atomically persist session")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
fn stored(path: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 64 * 1024, "Session file too large");
    Ok(serde_json::from_slice(&bytes)?)
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .with_context(|| format!("Bluesky response missing {name}"))
}
fn endpoint(service: &str, method: &str) -> String {
    format!("{}/xrpc/{method}", service.trim_end_matches('/'))
}
fn pds_service(fallback: &str, session: &Value) -> Result<String> {
    if let Some(services) = session["didDoc"]["service"].as_array() {
        let did = field(session, "did")?;
        for service in services {
            let id = service["id"].as_str().unwrap_or_default();
            if (id == "#atproto_pds" || id == format!("{did}#atproto_pds"))
                && service["type"] == "AtprotoPersonalDataServer"
            {
                let endpoint = field(service, "serviceEndpoint")?;
                let url = url::Url::parse(endpoint).context("Invalid Bluesky PDS endpoint")?;
                ensure!(
                    url.scheme() == "https"
                        && url.username().is_empty()
                        && url.password().is_none(),
                    "Bluesky PDS must use HTTPS without embedded credentials"
                );
                return Ok(endpoint.to_owned());
            }
        }
    }
    // Bluesky's entryway proxies authenticated repo requests for legacy sessions.
    Ok(fallback.to_owned())
}
pub struct Bluesky {
    http: Http,
    service: String,
    session: Value,
}
impl Bluesky {
    pub fn authenticate(config: &Config, http: &Http) -> Result<Self> {
        let service = config.service();
        let path = config.session_path();
        let stored_session = stored(&path).ok();
        let expected_account = stored_session
            .as_ref()
            .and_then(|s| s["did"].as_str())
            .map(str::to_owned);
        if let Some(mut session) = stored_session
            && let (Ok(refresh), Ok(did)) = (field(&session, "refreshJwt"), field(&session, "did"))
        {
            let expected_did = did.to_owned();
            let response = http.post(
                &endpoint(service, "com.atproto.server.refreshSession"),
                Some(&format!("Bearer {refresh}")),
                &json!({}),
            )?;
            if response.status().is_success() {
                let refreshed = http::json(response)?;
                field(&refreshed, "accessJwt")?;
                field(&refreshed, "refreshJwt")?;
                ensure!(
                    field(&refreshed, "did")? == expected_did,
                    "Bluesky refresh changed account DID; refusing publication"
                );
                if let (Some(old), Some(new)) = (session.as_object_mut(), refreshed.as_object()) {
                    // A missing DID document must not retain a previous PDS route.
                    old.remove("didDoc");
                    old.extend(new.clone());
                }
                persist(&path, &session)?;
                return Ok(Self {
                    http: http.clone(),
                    service: pds_service(service, &session)?,
                    session,
                });
            }
            // Do not turn transient server errors into repeated password logins.
            ensure!(
                matches!(response.status().as_u16(), 400 | 401),
                "Bluesky session refresh failed with HTTP {}",
                response.status().as_u16()
            );
        }
        let response = http.post(&endpoint(service,"com.atproto.server.createSession"),None,&json!({"identifier":config.required("BLUESKY_IDENTIFIER")?,"password":config.required("BLUESKY_PASSWORD")?}))?;
        let session = http::json(response)?;
        field(&session, "accessJwt")?;
        field(&session, "refreshJwt")?;
        let did = field(&session, "did")?;
        ensure!(
            expected_account
                .as_deref()
                .is_none_or(|expected| expected == did),
            "Bluesky login changed account DID; refusing publication"
        );
        persist(&path, &session)?;
        Ok(Self {
            http: http.clone(),
            service: pds_service(service, &session)?,
            session,
        })
    }
    fn auth(&self) -> Result<String> {
        Ok(format!("Bearer {}", field(&self.session, "accessJwt")?))
    }
    fn reference(&self, key: &str) -> Result<String> {
        Ok(format!(
            "https://bsky.app/profile/{}/post/{key}",
            field(&self.session, "did")?
        ))
    }
    pub fn reconcile(&self, key: &str, post: &Post) -> Result<Option<String>> {
        ensure!(
            valid_key(key),
            "Bluesky delivery requires a valid TID record key"
        );
        let mut url = url::Url::parse(&endpoint(&self.service, "com.atproto.repo.getRecord"))?;
        url.query_pairs_mut()
            .append_pair("repo", field(&self.session, "did")?)
            .append_pair("collection", "app.bsky.feed.post")
            .append_pair("rkey", key);
        let response = self.http.get(url.as_str(), Some(&self.auth()?))?;
        if response.status().is_success() {
            let existing = http::json(response)?;
            ensure!(
                existing["value"]["text"].as_str() == Some(post.text.as_str()),
                "Existing Bluesky record has unexpected text; refusing overwrite"
            );
            return Ok(Some(self.reference(key)?));
        }
        let status = response.status().as_u16();
        let error: Value =
            serde_json::from_slice(&http::body(response, 1024 * 1024)?).unwrap_or(Value::Null);
        if matches!(status, 400 | 404) && error["error"] == "RecordNotFound" {
            return Ok(None);
        }
        bail!("Bluesky record lookup failed with HTTP {status}; publication blocked")
    }
    pub fn upload(&self, bytes: &[u8], post: &Post) -> Result<Value> {
        ensure!(
            post.text.graphemes(true).count() <= 300 && post.text.len() <= 3000,
            "Bluesky text exceeds post limits"
        );
        ensure!(
            post.alt.graphemes(true).count() <= 2000 && post.alt.len() <= 20000,
            "Bluesky alt text exceeds limits"
        );
        let response = self.http.bytes(
            &endpoint(&self.service, "com.atproto.repo.uploadBlob"),
            &self.auth()?,
            "image/jpeg",
            bytes,
        )?;
        let result = http::json(response)?;
        ensure!(
            result["blob"].is_object(),
            "Bluesky upload response missing blob"
        );
        Ok(
            json!({"$type":"app.bsky.feed.post","text":post.text,"createdAt":now(),"embed":{"$type":"app.bsky.embed.images","images":[{"image":result["blob"],"alt":post.alt}]}}),
        )
    }
    // Caller persists publishing state before this request and treats every error as uncertain.
    pub fn create(&self, key: &str, record: &Value) -> Result<String> {
        let response = self.http.post(&endpoint(&self.service,"com.atproto.repo.createRecord"),Some(&self.auth()?),&json!({"repo":field(&self.session,"did")?,"collection":"app.bsky.feed.post","rkey":key,"record":record,"validate":true}))?;
        let result = http::json(response)?;
        let expected = format!(
            "at://{}/app.bsky.feed.post/{key}",
            field(&self.session, "did")?
        );
        ensure!(
            result["uri"].as_str() == Some(expected.as_str()),
            "Unexpected record URI in create response"
        );
        self.reference(key)
    }
}
