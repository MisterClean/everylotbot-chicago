use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Bluesky,
    Twitter,
}
impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bluesky => "bluesky",
            Self::Twitter => "twitter",
        }
    }
    pub fn column(self) -> &'static str {
        match self {
            Self::Bluesky => "posted_bluesky",
            Self::Twitter => "posted_twitter",
        }
    }
}

pub struct Config {
    pub database: PathBuf,
    pub platforms: Vec<Platform>,
    pub twitter_start: Option<String>,
    pub print_format: String,
    pub pitch: f64,
    pub zoom: f64,
    pub radius: u16,
    pub timeout: Duration,
    pub lease_seconds: u64,
    values: HashMap<String, String>,
}

pub fn valid_pin(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_digit())
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        // Read dotenv without mutating the process environment (unsafe in Rust 2024).
        let mut values = HashMap::new();
        match dotenvy::from_path_iter(path.unwrap_or_else(|| Path::new(".env"))) {
            Ok(iter) => {
                for entry in iter {
                    let (key, value) = entry.context("Invalid .env entry")?;
                    values.insert(key, value);
                }
            }
            Err(e) if e.not_found() && path.is_none() => {}
            Err(_) => bail!("Cannot read .env configuration"),
        }
        values.extend(std::env::vars());
        Self::from_values(values)
    }
    pub fn from_values(values: HashMap<String, String>) -> Result<Self> {
        let get = |key: &str, default: &str| {
            values
                .get(key)
                .cloned()
                .unwrap_or_else(|| default.to_owned())
        };
        let boolean = |key: &str, default: &str| -> Result<bool> {
            match get(key, default).to_lowercase().as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => bail!("{key} must be true or false"),
            }
        };
        let mut platforms = Vec::new();
        if boolean("ENABLE_BLUESKY", "true")? {
            platforms.push(Platform::Bluesky);
        }
        if boolean("ENABLE_TWITTER", "false")? {
            platforms.push(Platform::Twitter);
        }
        ensure!(
            !platforms.is_empty(),
            "At least one platform must be enabled"
        );
        let twitter_start = values
            .get("TWITTER_START_PIN10")
            .filter(|v| !v.is_empty())
            .cloned();
        if let Some(id) = &twitter_start {
            ensure!(valid_pin(id, 10), "TWITTER_START_PIN10 must be ten digits");
        }
        ensure!(
            !platforms.contains(&Platform::Twitter) || twitter_start.is_some(),
            "TWITTER_START_PIN10 is required to prevent historical backfill"
        );
        let database = PathBuf::from(get("DATABASE_PATH", "cook_county_lots.db"));
        ensure!(
            !database.as_os_str().is_empty(),
            "DATABASE_PATH must not be empty"
        );
        let raw_format = get("PRINT_FORMAT", "{address}");
        let print_format = if raw_format.len() >= 2
            && ((raw_format.starts_with('"') && raw_format.ends_with('"'))
                || (raw_format.starts_with('\'') && raw_format.ends_with('\'')))
        {
            raw_format[1..raw_format.len() - 1].to_owned()
        } else {
            raw_format
        };
        ensure!(!print_format.is_empty(), "PRINT_FORMAT must not be empty");
        let pitch: f64 = get("STREETVIEW_PITCH", "11.55")
            .parse()
            .context("Invalid STREETVIEW_PITCH")?;
        let zoom: f64 = get("STREETVIEW_ZOOM", "0.9")
            .parse()
            .context("Invalid STREETVIEW_ZOOM")?;
        ensure!(
            pitch.is_finite() && zoom.is_finite(),
            "Camera settings must be finite"
        );
        let radius = get("STREETVIEW_RADIUS_METERS", "500")
            .parse()
            .context("Invalid STREETVIEW_RADIUS_METERS")?;
        ensure!(
            (1..=1000).contains(&radius),
            "STREETVIEW_RADIUS_METERS must be 1..1000"
        );
        let timeout_ms: u64 = get("HTTP_TIMEOUT_MS", "30000")
            .parse()
            .context("Invalid HTTP_TIMEOUT_MS")?;
        ensure!(timeout_ms > 0, "HTTP_TIMEOUT_MS must be positive");
        let lease_seconds = get("LEASE_SECONDS", "840")
            .parse()
            .context("Invalid LEASE_SECONDS")?;
        ensure!(
            (60..=840).contains(&lease_seconds),
            "LEASE_SECONDS must be 60..840"
        );
        let service = get("BLUESKY_SERVICE", "https://bsky.social");
        let url = url::Url::parse(&service).context("Invalid BLUESKY_SERVICE")?;
        ensure!(
            matches!(url.scheme(), "https" | "http") && url.host_str().is_some(),
            "Invalid BLUESKY_SERVICE"
        );
        Ok(Self {
            database,
            platforms,
            twitter_start,
            print_format,
            pitch,
            zoom,
            radius,
            timeout: Duration::from_millis(timeout_ms),
            lease_seconds,
            values,
        })
    }
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
    pub fn required(&self, key: &str) -> Result<&str> {
        self.value(key)
            .with_context(|| format!("{key} is required"))
    }
    pub fn service(&self) -> &str {
        self.value("BLUESKY_SERVICE")
            .unwrap_or("https://bsky.social")
    }
    pub fn session_path(&self) -> PathBuf {
        self.value("BLUESKY_SESSION_PATH")
            .unwrap_or("var/bluesky-session.json")
            .into()
    }
    pub fn start(&self, platform: Platform) -> Option<&str> {
        if platform == Platform::Twitter {
            self.twitter_start.as_deref()
        } else {
            None
        }
    }
    pub fn validate_secrets(&self, platforms: &[Platform]) -> Result<()> {
        self.required("GOOGLE_API_KEY")?;
        for platform in platforms {
            let keys: &[&str] = match platform {
                Platform::Bluesky => &["BLUESKY_IDENTIFIER", "BLUESKY_PASSWORD"],
                Platform::Twitter => &[
                    "TWITTER_CONSUMER_KEY",
                    "TWITTER_CONSUMER_SECRET",
                    "TWITTER_ACCESS_TOKEN",
                    "TWITTER_ACCESS_TOKEN_SECRET",
                ],
            };
            for key in keys {
                self.required(key)?;
            }
        }
        Ok(())
    }
}
