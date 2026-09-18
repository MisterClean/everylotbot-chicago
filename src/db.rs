use crate::{
    config::{Config, Platform},
    domain::Lot,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use uuid::Uuid;

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub struct Database {
    pub connection: Connection,
}
pub struct Selection {
    pub lot: Lot,
    pub platforms: Vec<Platform>,
}
pub struct Delivery {
    pub state: String,
    pub key: Option<String>,
}
fn lot(row: &Row<'_>) -> rusqlite::Result<Lot> {
    Ok(Lot {
        id: row.get(0)?,
        address: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        lat: row.get::<_, Option<f64>>(2)?.unwrap_or_default(),
        lon: row.get::<_, Option<f64>>(3)?.unwrap_or_default(),
        posted_twitter: row.get(4)?,
        posted_bluesky: row.get(5)?,
    })
}
const FIELDS: &str = "id, address, lat, lon, posted_twitter, posted_bluesky";
impl Database {
    pub fn open(path: &Path, readonly: bool) -> Result<Self> {
        let flags = if readonly {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        let connection = Connection::open_with_flags(path, flags)
            .context("Cannot open existing parcel database")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA cache_size=-2048; PRAGMA temp_store=FILE;",
        )?;
        if readonly {
            connection.execute_batch("PRAGMA query_only=ON;")?;
        }
        let db = Self { connection };
        db.check_columns(
            "lots",
            &[
                "id",
                "address",
                "lat",
                "lon",
                "posted_twitter",
                "posted_bluesky",
            ],
        )?;
        Ok(db)
    }
    fn check_columns(&self, table: &str, required: &[&str]) -> Result<()> {
        let names: Vec<String> = self
            .connection
            .prepare(&format!("PRAGMA table_info({table})"))?
            .query_map([], |r| r.get(1))?
            .collect::<rusqlite::Result<_>>()?;
        for column in required {
            ensure!(
                names.iter().any(|n| n == column),
                "Database is missing required {table}.{column}"
            );
        }
        Ok(())
    }
    pub fn check_v1(&self) -> Result<()> {
        for (table, columns) in [
            ("schema_migrations", &["version", "applied_at"][..]),
            (
                "platform_state",
                &["platform", "start_after_id", "updated_at"][..],
            ),
            (
                "post_deliveries",
                &[
                    "lot_id",
                    "platform",
                    "state",
                    "deterministic_key",
                    "post_ref",
                    "attempt_count",
                    "last_error",
                    "started_at",
                    "confirmed_at",
                    "updated_at",
                ][..],
            ),
            (
                "bot_runs",
                &[
                    "run_id",
                    "application_version",
                    "selected_lot_id",
                    "started_at",
                    "completed_at",
                    "outcome",
                    "error_code",
                ][..],
            ),
            ("bot_leases", &["name", "owner", "expires_at"][..]),
        ] {
            self.check_columns(table, columns)?;
        }
        let version: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=1)",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            version,
            "Database v1 migration is required; posting never migrates automatically"
        );
        Ok(())
    }
    pub fn migrate(&self) -> Result<()> {
        if self.check_v1().is_ok() {
            return Ok(());
        }
        self.connection
            .execute_batch(include_str!("schema-v1.sql"))?;
        self.check_v1()
    }
    pub fn high_water(&self, platform: Platform, fallback: Option<&str>) -> Result<Option<String>> {
        let high: Option<String> = self.connection.query_row(
            &format!(
                "SELECT MAX(id) FROM lots WHERE {} != '0'",
                platform.column()
            ),
            [],
            |r| r.get(0),
        )?;
        if high.is_some() {
            return Ok(high);
        }
        let has_state: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='platform_state')", [], |r| r.get(0))?;
        if has_state {
            let start: Option<String> = self
                .connection
                .query_row(
                    "SELECT start_after_id FROM platform_state WHERE platform=?",
                    [platform.name()],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if start.is_some() {
                return Ok(start);
            }
        }
        Ok(fallback.map(str::to_owned))
    }
    pub fn next(&self, platform: Platform, fallback: Option<&str>) -> Result<Option<Lot>> {
        let cursor = self
            .high_water(platform, fallback)?
            .unwrap_or_else(|| "0".to_owned());
        Ok(self
            .connection
            .query_row(
                &format!(
                    "SELECT {FIELDS} FROM lots WHERE id > ? AND {}='0' ORDER BY id LIMIT 1",
                    platform.column()
                ),
                [cursor],
                lot,
            )
            .optional()?)
    }
    pub fn select(
        &self,
        config: &Config,
        platforms: &[Platform],
        id: Option<&str>,
    ) -> Result<Option<Selection>> {
        if let Some(id) = id {
            let Some(lot) = self
                .connection
                .query_row(&format!("SELECT {FIELDS} FROM lots WHERE id=?"), [id], lot)
                .optional()?
            else {
                return Ok(None);
            };
            let pending: Vec<_> = platforms
                .iter()
                .copied()
                .filter(|p| match p {
                    Platform::Bluesky => lot.posted_bluesky == "0",
                    Platform::Twitter => lot.posted_twitter == "0",
                })
                .collect();
            return Ok(if pending.is_empty() {
                None
            } else {
                Some(Selection {
                    lot,
                    platforms: pending,
                })
            });
        }
        let mut selected: Option<Selection> = None;
        for &platform in platforms {
            if let Some(candidate) = self.next(platform, config.start(platform))? {
                match &mut selected {
                    Some(s) if s.lot.id == candidate.id => s.platforms.push(platform),
                    Some(s) if s.lot.id < candidate.id => {}
                    _ => {
                        selected = Some(Selection {
                            lot: candidate,
                            platforms: vec![platform],
                        })
                    }
                }
            }
        }
        Ok(selected)
    }
    pub fn lease(&self, ttl: u64) -> Result<String> {
        let owner = Uuid::new_v4().to_string();
        let tx = self.connection.unchecked_transaction()?;
        let changed = tx.execute("INSERT INTO bot_leases(name,owner,expires_at) VALUES ('post-next',?,unixepoch()+?) ON CONFLICT(name) DO UPDATE SET owner=excluded.owner,expires_at=excluded.expires_at WHERE bot_leases.expires_at <= unixepoch()", params![owner, i64::try_from(ttl)?])?;
        ensure!(
            changed == 1,
            "Another post-next invocation holds the database lease"
        );
        tx.commit()?;
        Ok(owner)
    }
    pub fn renew(&self, owner: &str, ttl: u64) -> Result<()> {
        let changed = self.connection.execute("UPDATE bot_leases SET expires_at=unixepoch()+? WHERE name='post-next' AND owner=? AND expires_at>unixepoch()", params![i64::try_from(ttl)?, owner])?;
        ensure!(
            changed == 1,
            "Posting lease expired or was lost; refusing publication"
        );
        Ok(())
    }
    pub fn release(&self, owner: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM bot_leases WHERE name='post-next' AND owner=?",
            [owner],
        )?;
        Ok(())
    }
    pub fn initialize_twitter_start(&self, start: Option<&str>) -> Result<()> {
        if let Some(start) = start
            && self.high_water(Platform::Twitter, None)?.is_none()
        {
            self.connection.execute(
                "INSERT INTO platform_state(platform,start_after_id,updated_at) VALUES ('twitter',?,?) ON CONFLICT(platform) DO UPDATE SET start_after_id=excluded.start_after_id,updated_at=excluded.updated_at",
                params![start, now()],
            )?;
        }
        Ok(())
    }
    pub fn start_run(&self) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.connection.execute(
            "INSERT INTO bot_runs(run_id,application_version,started_at) VALUES (?,?,?)",
            params![id, env!("CARGO_PKG_VERSION"), now()],
        )?;
        Ok(id)
    }
    pub fn finish_run(&self, id: &str, outcome: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE bot_runs SET completed_at=?,outcome=?,error_code=? WHERE run_id=?",
            params![
                now(),
                outcome,
                if outcome == "failed" {
                    Some("RUN_FAILURE")
                } else {
                    None
                },
                id
            ],
        )?;
        Ok(())
    }
    pub fn delivery(&self, id: &str, platform: Platform) -> Result<Option<Delivery>> {
        Ok(self
            .connection
            .query_row(
                "SELECT state,deterministic_key FROM post_deliveries WHERE lot_id=? AND platform=?",
                params![id, platform.name()],
                |r| {
                    Ok(Delivery {
                        state: r.get(0)?,
                        key: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }
    pub fn begin(&self, id: &str, platform: Platform, key: Option<&str>) -> Result<()> {
        self.connection.execute("INSERT INTO post_deliveries(lot_id,platform,state,deterministic_key,attempt_count,started_at,updated_at) VALUES (?,?,'publishing',?,1,?,?) ON CONFLICT(lot_id,platform) DO UPDATE SET state='publishing',deterministic_key=excluded.deterministic_key,attempt_count=post_deliveries.attempt_count+1,last_error=NULL,started_at=excluded.started_at,updated_at=excluded.updated_at", params![id, platform.name(), key, now(), now()])?;
        Ok(())
    }
    pub fn confirm(&self, id: &str, platform: Platform, reference: &str) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let changed = tx.execute(
            &format!(
                "UPDATE lots SET {}=? WHERE id=? AND {}='0'",
                platform.column(),
                platform.column()
            ),
            params![reference, id],
        )?;
        ensure!(
            changed == 1,
            "Parcel confirmation conflict; delivery requires reconciliation"
        );
        let changed = tx.execute("UPDATE post_deliveries SET state='confirmed',post_ref=?,confirmed_at=?,updated_at=?,last_error=NULL WHERE lot_id=? AND platform=?", params![reference, now(), now(), id, platform.name()])?;
        ensure!(
            changed == 1,
            "Missing durable delivery; confirmation rolled back"
        );
        tx.commit()?;
        Ok(())
    }
    pub fn fail(&self, id: &str, platform: Platform, message: &str, uncertain: bool) -> Result<()> {
        let message: String = message.chars().take(2000).collect();
        self.connection.execute("UPDATE post_deliveries SET state=?,last_error=?,updated_at=? WHERE lot_id=? AND platform=?", params![if uncertain { "unknown" } else { "failed" }, message, now(), id, platform.name()])?;
        Ok(())
    }
    pub fn audit(&self, config: &Config) -> Result<Value> {
        let integrity: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        let (total, bluesky, twitter, first, last): (i64,i64,i64,Option<String>,Option<String>) = self.connection.query_row("SELECT COUNT(*), COALESCE(SUM(posted_bluesky NOT IN ('0','1')),0), COALESCE(SUM(posted_twitter != '0'),0), MIN(CASE WHEN posted_bluesky != '0' THEN id END), MAX(CASE WHEN posted_bluesky != '0' THEN id END) FROM lots", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
        let (skipped, gaps, remaining): (i64, i64, i64) = if let (Some(first), Some(last)) =
            (&first, &last)
        {
            self.connection.query_row("SELECT COALESCE(SUM(id < ? AND posted_bluesky='0'),0),COALESCE(SUM(id BETWEEN ? AND ? AND posted_bluesky='0'),0),COALESCE(SUM(id > ? AND posted_bluesky='0'),0) FROM lots", params![first,first,last,last], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?
        } else {
            (0, 0, total)
        };
        let mut next = serde_json::Map::new();
        for &platform in &config.platforms {
            next.insert(
                platform.name().to_owned(),
                serde_json::to_value(self.next(platform, config.start(platform))?)?,
            );
        }
        Ok(
            json!({"integrity":integrity,"total":total,"blueskyConfirmed":bluesky,"twitterConfirmed":twitter,"firstBlueskyId":first,"lastBlueskyId":last,"skippedBeforeBlueskyStart":skipped,"gapsInBlueskyRun":gaps,"remainingAfterBlueskyCursor":remaining,"nextByPlatform":next}),
        )
    }
}
