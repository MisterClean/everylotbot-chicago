use crate::{
    app::log,
    config::{Config, valid_pin},
    db::Database,
    domain::coordinates,
    http::{self, Http},
};
use anyhow::{Context, Result, anyhow, ensure};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, time::Duration};
const ADDRESSES: &str = "https://datacatalog.cookcountyil.gov/resource/3723-97qp.csv";
const CENTROIDS: &str = "https://datacatalog.cookcountyil.gov/resource/nj4t-kc8j.json";
const MISSING: &str = "TRIM(UPPER(COALESCE(address,''))) IN ('','CHICAGO, IL',', CHICAGO, IL')";
#[derive(Deserialize)]
struct SourceRow {
    pin: String,
    pin10: String,
    prop_address_full: String,
    prop_address_city_name: String,
    prop_address_state: String,
    prop_address_zipcode_1: String,
}

pub fn stage_csv(db: &Database, input: impl Read) -> Result<(usize, usize)> {
    let tx = db.connection.unchecked_transaction()?;
    let mut raw = 0;
    let mut valid = 0;
    {
        let mut insert=tx.prepare("INSERT INTO import_lots(id,pin14,address) VALUES (?,?,?) ON CONFLICT(id) DO UPDATE SET pin14=CASE WHEN excluded.address!='' THEN excluded.pin14 ELSE import_lots.pin14 END,address=CASE WHEN excluded.address!='' THEN excluded.address ELSE import_lots.address END")?;
        let mut reader = csv::Reader::from_reader(input);
        for row in reader.deserialize::<SourceRow>() {
            let row = row.context("Invalid Cook County CSV row")?;
            raw += 1;
            if !valid_pin(&row.pin10, 10) || !valid_pin(&row.pin, 14) {
                continue;
            }
            let street = row.prop_address_full.trim();
            let address = if street.is_empty() {
                String::new()
            } else {
                format!(
                    "{street}, {}, {} {}",
                    row.prop_address_city_name.trim(),
                    row.prop_address_state.trim(),
                    row.prop_address_zipcode_1.trim()
                )
                .trim_end()
                .trim_end_matches(',')
                .to_owned()
            };
            insert.execute(params![row.pin10, row.pin, address])?;
            valid += 1;
        }
    }
    tx.commit()?;
    Ok((raw, valid))
}
pub fn create_staging(db: &Database) -> Result<()> {
    db.connection.execute_batch("CREATE TEMP TABLE import_lots(id TEXT PRIMARY KEY,pin14 TEXT NOT NULL,address TEXT NOT NULL)")?;
    Ok(())
}
pub fn apply_staging(db: &Database) -> Result<i64> {
    let count: i64 = db
        .connection
        .query_row("SELECT COUNT(*) FROM import_lots", [], |r| r.get(0))?;
    ensure!(
        count > 0,
        "Import produced zero valid lots; refusing to modify the live table"
    );
    let tx = db.connection.unchecked_transaction()?;
    tx.execute_batch("INSERT INTO lots(id,address,lat,lon,posted_twitter,posted_bluesky) SELECT id,address,0.0,0.0,'0','0' FROM import_lots WHERE true ON CONFLICT(id) DO UPDATE SET address=CASE WHEN TRIM(excluded.address)!='' THEN excluded.address ELSE lots.address END")?;
    tx.commit()?;
    Ok(count)
}
pub fn ingest(config: &Config, year: &str, city: &str, batch: usize) -> Result<Value> {
    ingest_from(config, year, city, batch, ADDRESSES)
}
pub fn ingest_from(
    config: &Config,
    year: &str,
    city: &str,
    batch: usize,
    source: &str,
) -> Result<Value> {
    ensure!(valid_pin(year, 4), "year must be four digits");
    ensure!(
        !city.is_empty()
            && city
                .bytes()
                .all(|b| b.is_ascii_alphabetic() || b" .-".contains(&b)),
        "city contains unsupported characters"
    );
    ensure!((1..=50000).contains(&batch), "batch size must be 1..50000");
    let token = config.required("CHICAGO_DATA_PORTAL_TOKEN")?;
    let db = Database::open(&config.database, false)?;
    db.check_v1()?;
    create_staging(&db)?;
    let http = Http::new(config.timeout.max(Duration::from_secs(120)));
    let mut offset = 0;
    let mut fetched = 0;
    loop {
        let query = format!(
            "SELECT pin, pin10, year, prop_address_full, prop_address_city_name, prop_address_state, prop_address_zipcode_1 WHERE year IN ('{year}') AND caseless_one_of(prop_address_city_name, '{city}', '{}') ORDER BY pin ASC LIMIT {batch} OFFSET {offset}",
            city.to_lowercase()
        );
        let mut url = url::Url::parse(source)?;
        url.query_pairs_mut().append_pair("$query", &query);
        let mut response = http
            .agent
            .get(url.as_str())
            .header("X-App-Token", token)
            .call()
            .map_err(|_| anyhow!("Cook County request failed or timed out"))?;
        ensure!(
            response.status().is_success(),
            "Cook County API returned HTTP {}",
            response.status().as_u16()
        );
        // Bound malformed fields while normally holding only one CSV record in RAM.
        let (raw, valid) = stage_csv(
            &db,
            response
                .body_mut()
                .with_config()
                .limit(16 * 1024 * 1024)
                .reader(),
        )?;
        fetched += valid;
        log("ingest_page", json!({"offset":offset,"fetched":valid}));
        if raw < batch {
            break;
        }
        offset += batch;
    }
    let staged = apply_staging(&db)?;
    Ok(json!({"fetched":fetched,"staged":staged}))
}
#[derive(Deserialize)]
struct Centroid {
    #[serde(default)]
    pin10: String,
    lat: Option<String>,
    lon: Option<String>,
}
pub fn enrich(config: &Config, batch: usize) -> Result<Value> {
    enrich_from(config, batch, CENTROIDS)
}
pub fn enrich_from(config: &Config, batch: usize, source: &str) -> Result<Value> {
    ensure!(
        (1..=100).contains(&batch),
        "centroid batch size must be 1..100"
    );
    let token = config.required("CHICAGO_DATA_PORTAL_TOKEN")?;
    let db = Database::open(&config.database, false)?;
    db.check_v1()?;
    db.connection.execute_batch(&format!("CREATE TEMP TABLE centroid_candidates(id TEXT PRIMARY KEY); INSERT INTO centroid_candidates SELECT id FROM lots WHERE posted_bluesky='0' AND {MISSING}; CREATE TEMP TABLE centroid_matches(id TEXT PRIMARY KEY,lat REAL,lon REAL);"))?;
    let http = Http::new(config.timeout.max(Duration::from_secs(120)));
    let mut cursor = String::new();
    let mut eligible = 0;
    let mut matched = 0;
    loop {
        let ids: Vec<String> = db
            .connection
            .prepare("SELECT id FROM centroid_candidates WHERE id>? ORDER BY id LIMIT ?")?
            .query_map(params![cursor, i64::try_from(batch)?], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if ids.is_empty() {
            break;
        }
        ensure!(
            ids.iter().all(|id| valid_pin(id, 10)),
            "Invalid PIN10 in centroid candidate set"
        );
        let pins = ids
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT pin10, year, lat, lon WHERE pin10 IN ({pins}) AND lat IS NOT NULL AND lon IS NOT NULL ORDER BY pin10 ASC, year DESC LIMIT 50000"
        );
        let mut url = url::Url::parse(source)?;
        url.query_pairs_mut().append_pair("$query", &query);
        let response = http
            .agent
            .get(url.as_str())
            .header("X-App-Token", token)
            .call()
            .map_err(|_| anyhow!("Cook County centroid request failed or timed out"))?;
        ensure!(
            response.status().is_success(),
            "Cook County centroid API returned HTTP {}",
            response.status().as_u16()
        );
        let rows: Vec<Centroid> = serde_json::from_slice(&http::body(response, 4 * 1024 * 1024)?)
            .context("Invalid centroid response")?;
        ensure!(
            rows.len() < 50000,
            "Centroid query reached its row limit; reduce batch size to avoid truncated data"
        );
        let tx = db.connection.unchecked_transaction()?;
        let mut page_matched = 0;
        for row in rows {
            if ids.binary_search(&row.pin10).is_err() {
                continue;
            }
            let Some((lat, lon)) = row
                .lat
                .as_deref()
                .and_then(|s| s.parse::<f64>().ok())
                .zip(row.lon.as_deref().and_then(|s| s.parse::<f64>().ok()))
            else {
                continue;
            };
            if !coordinates(lat, lon) {
                continue;
            }
            page_matched += tx.execute(
                "INSERT OR IGNORE INTO centroid_matches VALUES (?,?,?)",
                params![row.pin10, lat, lon],
            )?;
        }
        tx.commit()?;
        eligible += ids.len();
        matched += page_matched;
        log(
            "centroid_page",
            json!({"requested":ids.len(),"matched":page_matched}),
        );
        cursor = ids.last().context("Missing batch cursor")?.clone();
    }
    let tx = db.connection.unchecked_transaction()?;
    let updated=tx.execute(&format!("UPDATE lots SET lat=centroid_matches.lat,lon=centroid_matches.lon FROM centroid_matches WHERE lots.id=centroid_matches.id AND posted_bluesky='0' AND {MISSING}"),[])?;
    tx.commit()?;
    let missing:Vec<String>=db.connection.prepare("SELECT id FROM centroid_candidates WHERE id NOT IN (SELECT id FROM centroid_matches) ORDER BY id LIMIT 100")?.query_map([],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(
        json!({"eligible":eligible,"matched":matched,"updated":updated,"missing":missing,"missingCount":eligible-matched,"missingTruncated":eligible-matched>100}),
    )
}
