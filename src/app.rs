use crate::{
    bluesky::{self, Bluesky},
    config::{Config, Platform},
    db::Database,
    domain::compose,
    http::Http,
    streetview, twitter,
};
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::time::Duration;
pub fn log(event: &str, fields: Value) {
    let mut object = fields.as_object().cloned().unwrap_or_default();
    object.insert("event".to_owned(), json!(event));
    object.insert("timestamp".to_owned(), json!(crate::db::now()));
    object.insert(
        "level".to_owned(),
        json!(if event.ends_with("_failed") {
            "error"
        } else {
            "info"
        }),
    );
    println!("{}", Value::Object(object));
}
pub fn run(
    config: &Config,
    platforms: &[Platform],
    specific: Option<&str>,
    dry_run: bool,
) -> Result<Value> {
    run_with_source(config, platforms, specific, dry_run, None)
}
pub(crate) fn run_with_source(
    config: &Config,
    platforms: &[Platform],
    specific: Option<&str>,
    dry_run: bool,
    image_source: Option<&str>,
) -> Result<Value> {
    for platform in platforms {
        ensure!(
            config.platforms.contains(platform),
            "{} is not enabled",
            platform.name()
        );
    }
    let db = Database::open(&config.database, dry_run)?;
    if dry_run {
        let Some(selection) = db.select(config, platforms, specific)? else {
            return Ok(json!({"outcome":"no-lot"}));
        };
        let post = compose(&selection.lot, &config.print_format)?;
        log(
            "dry_run",
            json!({"lotId":selection.lot.id,"platforms":selection.platforms,"text":post.text,"alt":post.alt}),
        );
        return Ok(
            json!({"outcome":"dry-run","lotId":selection.lot.id,"platforms":selection.platforms}),
        );
    }
    config.validate_secrets(platforms)?;
    db.check_v1()?;
    let owner = db.lease(config.lease_seconds)?;
    let result = (|| {
        if config.platforms.contains(&Platform::Twitter) {
            db.initialize_twitter_start(config.twitter_start.as_deref())?;
        }
        let run_id = db.start_run()?;
        let outcome = publish(
            config,
            &db,
            platforms,
            specific,
            &owner,
            &run_id,
            image_source,
        );
        let status = match &outcome {
            Ok(value) => value["outcome"].as_str().unwrap_or("failed"),
            Err(_) => "failed",
        };
        let finished = db.finish_run(&run_id, status);
        match outcome {
            Ok(value) => {
                finished?;
                Ok(value)
            }
            Err(error) => Err(error),
        }
    })();
    let released = db.release(&owner);
    match result {
        Ok(value) => {
            released?;
            Ok(value)
        }
        Err(error) => Err(error),
    }
}
fn publish(
    config: &Config,
    db: &Database,
    platforms: &[Platform],
    specific: Option<&str>,
    owner: &str,
    run_id: &str,
    image_source: Option<&str>,
) -> Result<Value> {
    let Some(selection) = db.select(config, platforms, specific)? else {
        return Ok(json!({"outcome":"no-lot"}));
    };
    let lot = &selection.lot;
    db.connection.execute(
        "UPDATE bot_runs SET selected_lot_id=? WHERE run_id=?",
        rusqlite::params![lot.id, run_id],
    )?;
    let post = compose(lot, &config.print_format)?;
    // Each network operation completes well before a renewed lease can expire.
    let http = Http::new(
        config
            .timeout
            .min(Duration::from_secs(config.lease_seconds / 4)),
    );
    let mut image = None;
    let mut failed = false;
    for &platform in &selection.platforms {
        let prior = db.delivery(&lot.id, platform)?;
        let was_uncertain = prior
            .as_ref()
            .is_some_and(|d| matches!(d.state.as_str(), "publishing" | "unknown"));
        if platform == Platform::Twitter && was_uncertain {
            log(
                "post_failed",
                json!({"lotId":lot.id,"platform":platform,"uncertain":true,"message":"Uncertain Twitter delivery requires manual reconciliation"}),
            );
            failed = true;
            continue;
        }
        let key = if platform == Platform::Bluesky {
            match prior.as_ref().and_then(|d| d.key.as_deref()) {
                Some(key) if bluesky::valid_key(key) => Some(key.to_owned()),
                _ if was_uncertain => bail!(
                    "Uncertain Bluesky delivery has no valid stored key; manual reconciliation required"
                ),
                _ => Some(bluesky::new_key()?),
            }
        } else {
            None
        };
        db.renew(owner, config.lease_seconds)?;
        db.begin(&lot.id, platform, key.as_deref())?;
        let mut write_started = false;
        let result = (|| {
            let publisher = if platform == Platform::Bluesky {
                Some(Bluesky::authenticate(config, &http)?)
            } else {
                None
            };
            if let (Some(publisher), Some(key)) = (&publisher, &key) {
                db.renew(owner, config.lease_seconds)?;
                if let Some(reference) = publisher.reconcile(key, &post)? {
                    return Ok(reference);
                }
            }
            if image.is_none() {
                db.renew(owner, config.lease_seconds)?;
                let url = streetview::build_url(config, lot)?;
                image = Some(streetview::fetch(
                    &http,
                    image_source.unwrap_or(url.as_str()),
                )?);
            }
            let bytes = image
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Street View image unavailable"))?;
            db.renew(owner, config.lease_seconds)?;
            let reference = match (&publisher, &key) {
                (Some(publisher), Some(key)) => {
                    let record = publisher.upload(bytes, &post)?;
                    db.renew(owner, config.lease_seconds)?;
                    write_started = true;
                    publisher.create(key, &record)?
                }
                _ => {
                    let media = twitter::upload(config, &http, bytes)?;
                    db.renew(owner, config.lease_seconds)?;
                    write_started = true;
                    twitter::create(config, &http, &post, &media)?
                }
            };
            Ok::<_, anyhow::Error>(reference)
        })();
        match result {
            Ok(reference) => {
                // A local confirmation failure must preserve an uncertain delivery.
                if let Err(error) = db.confirm(&lot.id, platform, &reference) {
                    let _ = db.fail(
                        &lot.id,
                        platform,
                        "Local confirmation failed; reconcile remote record",
                        true,
                    );
                    return Err(error);
                }
                log(
                    "post_confirmed",
                    json!({"runId":run_id,"lotId":lot.id,"platform":platform,"postRef":reference}),
                );
            }
            Err(error) => {
                let uncertain = write_started || was_uncertain;
                db.fail(&lot.id, platform, &error.to_string(), uncertain)?;
                log(
                    "post_failed",
                    json!({"runId":run_id,"lotId":lot.id,"platform":platform,"uncertain":uncertain,"message":error.to_string()}),
                );
                failed = true;
            }
        }
    }
    ensure!(!failed, "Posting failed for lot {}", lot.id);
    Ok(json!({"outcome":"posted","lotId":lot.id,"platforms":selection.platforms}))
}
