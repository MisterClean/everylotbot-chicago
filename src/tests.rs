#![expect(
    clippy::unwrap_used,
    reason = "Tests fail immediately on fixture and mock errors."
)]
use super::*;
use anyhow::Result;
use base64::Engine;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn config(path: &Path, extra: &[(&str, &str)]) -> Config {
    let mut values = HashMap::from([(
        "DATABASE_PATH".to_owned(),
        path.to_string_lossy().into_owned(),
    )]);
    values.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    Config::from_values(values).unwrap()
}
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("lots.db")).unwrap();
    db.execute_batch(include_str!("../schema.sql")).unwrap();
    db.execute_batch("INSERT INTO lots VALUES
      ('0428206041','1 N EARLY ST, CHICAGO, IL',0,0,'0','0'),
      ('1431213017','2 N SKIPPED ST, CHICAGO, IL',0,0,'0','0'),
      ('1431213018','2023 N DAMEN AVE, CHICAGO, IL',0,0,'0','1'),
      ('1431213019','2019 N DAMEN AVE, CHICAGO, IL',0,0,'0','0'),
      ('1431213020','2017 N DAMEN AVE 1, CHICAGO, IL',0,0,'0','https://bsky.app/profile/did:plc:test/post/old'),
      ('1431213021','2015 N DAMEN AVE, CHICAGO, IL',0,0,'0','0'),
      ('1431213024','2038 N WINCHESTER AVE, CHICAGO, IL',0,0,'0','0');").unwrap();
    dir
}
fn database(dir: &TempDir) -> db::Database {
    db::Database::open(&dir.path().join("lots.db"), false).unwrap()
}
struct Reply {
    path: &'static str,
    status: u16,
    content_type: &'static str,
    body: String,
}
fn reply(path: &'static str, status: u16, value: Value) -> Reply {
    Reply {
        path,
        status,
        content_type: "application/json",
        body: value.to_string(),
    }
}
struct Mock {
    url: String,
    handle: thread::JoinHandle<Vec<String>>,
}
impl Mock {
    fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "missing mock request {}",
                                reply.path
                            );
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0u8; 4096];
                let header_end = loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                if String::from_utf8_lossy(&bytes[..header_end])
                    .to_lowercase()
                    .contains("transfer-encoding: chunked")
                {
                    let mut decoded = Vec::new();
                    let mut offset = header_end;
                    loop {
                        let line_end = loop {
                            if let Some(end) = bytes[offset..].windows(2).position(|w| w == b"\r\n")
                            {
                                break offset + end;
                            }
                            let n = stream.read(&mut buf).unwrap();
                            assert!(n > 0);
                            bytes.extend_from_slice(&buf[..n]);
                        };
                        let size = usize::from_str_radix(
                            std::str::from_utf8(&bytes[offset..line_end]).unwrap(),
                            16,
                        )
                        .unwrap();
                        offset = line_end + 2;
                        while bytes.len() < offset + size + 2 {
                            let n = stream.read(&mut buf).unwrap();
                            assert!(n > 0);
                            bytes.extend_from_slice(&buf[..n]);
                        }
                        if size == 0 {
                            break;
                        }
                        decoded.extend_from_slice(&bytes[offset..offset + size]);
                        offset += size + 2;
                    }
                    bytes.truncate(header_end);
                    bytes.extend_from_slice(&decoded);
                }
                let request = String::from_utf8_lossy(&bytes).into_owned();
                assert!(
                    request.lines().next().unwrap().contains(reply.path),
                    "unexpected request: {}",
                    request.lines().next().unwrap()
                );
                requests.push(request);
                write!(stream,"HTTP/1.1 {} Status\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",reply.status,reply.content_type,reply.body.len(),reply.body).unwrap();
            }
            requests
        });
        Self { url, handle }
    }
    fn finish(self) -> Vec<String> {
        self.handle.join().unwrap()
    }
}
fn session(exp: i64) -> Value {
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json!({"exp":exp}).to_string());
    json!({"did":"did:plc:test","handle":"test.bsky.social","accessJwt":format!("header.{payload}.signature"),"refreshJwt":"refresh-secret","active":true})
}
fn posting_config(dir: &TempDir, service: &str) -> Config {
    config(
        &dir.path().join("lots.db"),
        &[
            ("GOOGLE_API_KEY", "secret-google"),
            ("BLUESKY_IDENTIFIER", "test.bsky.social"),
            ("BLUESKY_PASSWORD", "secret-password"),
            ("BLUESKY_SERVICE", service),
            (
                "BLUESKY_SESSION_PATH",
                dir.path().join("session.json").to_str().unwrap(),
            ),
            ("HTTP_TIMEOUT_MS", "1000"),
        ],
    )
}
fn save_session(dir: &TempDir) {
    bluesky::persist(
        &dir.path().join("session.json"),
        &session(chrono::Utc::now().timestamp() + 3600),
    )
    .unwrap();
}

#[test]
fn preserves_high_water_and_historical_sentinel() {
    let dir = fixture();
    let db = database(&dir);
    let c = config(&dir.path().join("lots.db"), &[]);
    let audit = db.audit(&c).unwrap();
    assert_eq!(audit["nextByPlatform"]["bluesky"]["id"], "1431213021");
    assert_eq!(
        (
            audit["skippedBeforeBlueskyStart"].as_i64(),
            audit["gapsInBlueskyRun"].as_i64(),
            audit["blueskyConfirmed"].as_i64()
        ),
        (Some(2), Some(1), Some(1))
    );
}
#[test]
fn explicit_id_can_target_gap_but_cannot_repost_confirmed() {
    let dir = fixture();
    let db = database(&dir);
    let c = config(&dir.path().join("lots.db"), &[]);
    assert!(
        db.select(&c, &[Platform::Bluesky], Some("1431213019"))
            .unwrap()
            .is_some()
    );
    assert!(
        db.select(&c, &[Platform::Bluesky], Some("1431213020"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn lagging_secondary_platform_uses_its_own_cursor() {
    let dir = fixture();
    let db = database(&dir);
    let c = config(
        &dir.path().join("lots.db"),
        &[
            ("ENABLE_TWITTER", "true"),
            ("TWITTER_START_PIN10", "1431213019"),
        ],
    );
    let selection = db.select(&c, &c.platforms, None).unwrap().unwrap();
    assert_eq!(
        (selection.lot.id, selection.platforms),
        ("1431213020".to_owned(), vec![Platform::Twitter])
    );
}
#[test]
fn migration_is_idempotent_and_preserves_existing_database_bytes() {
    let dir = fixture();
    let db = database(&dir);
    db.migrate().unwrap();
    drop(db);
    let path = dir.path().join("lots.db");
    let before = fs::read(&path).unwrap();
    let db = database(&dir);
    db.migrate().unwrap();
    drop(db);
    assert_eq!(before, fs::read(path).unwrap());
}
#[test]
fn audit_and_dry_run_do_not_write_or_require_secrets() {
    let dir = fixture();
    let path = dir.path().join("lots.db");
    let before = fs::read(&path).unwrap();
    let c = config(&path, &[]);
    assert_eq!(
        app::run(&c, &c.platforms, None, true).unwrap()["lotId"],
        "1431213021"
    );
    db::Database::open(&path, true).unwrap().audit(&c).unwrap();
    assert_eq!(before, fs::read(path).unwrap());
}
#[test]
fn posting_does_not_migrate_legacy_database() {
    let dir = fixture();
    let path = dir.path().join("lots.db");
    let before = fs::read(&path).unwrap();
    let c = posting_config(&dir, "http://127.0.0.1:1");
    assert!(app::run(&c, &c.platforms, None, false).is_err());
    assert_eq!(before, fs::read(path).unwrap());
}
#[test]
fn missing_database_is_never_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.db");
    assert!(db::Database::open(&path, false).is_err());
    assert!(!path.exists());
}
#[test]
fn confirmation_rolls_back_when_delivery_is_missing() {
    let dir = fixture();
    let db = database(&dir);
    db.migrate().unwrap();
    assert!(
        db.confirm("1431213021", Platform::Bluesky, "remote")
            .is_err()
    );
    assert_eq!(
        db.next(Platform::Bluesky, None).unwrap().unwrap().id,
        "1431213021"
    );
}
#[test]
fn confirmation_updates_both_legacy_and_delivery_state() {
    let dir = fixture();
    let db = database(&dir);
    db.migrate().unwrap();
    db.begin("1431213021", Platform::Bluesky, Some("3jzfcijpj2z2a"))
        .unwrap();
    db.confirm("1431213021", Platform::Bluesky, "remote")
        .unwrap();
    assert_eq!(
        db.high_water(Platform::Bluesky, None).unwrap().as_deref(),
        Some("1431213021")
    );
    assert_eq!(
        db.delivery("1431213021", Platform::Bluesky)
            .unwrap()
            .unwrap()
            .state,
        "confirmed"
    );
}
#[test]
fn leases_exclude_other_connections_and_expired_owner_cannot_renew() {
    let dir = fixture();
    let db = database(&dir);
    db.migrate().unwrap();
    let other = database(&dir);
    let owner = db.lease(60).unwrap();
    assert!(other.lease(60).is_err());
    db.connection
        .execute("UPDATE bot_leases SET expires_at=0", [])
        .unwrap();
    let new = other.lease(60).unwrap();
    assert!(db.renew(&owner, 60).is_err());
    db.release(&owner).unwrap();
    assert!(db.lease(60).is_err());
    other.release(&new).unwrap();
    assert!(db.lease(60).is_ok());
}
#[test]
fn keys_follow_tid_spec_and_are_monotonic() {
    let a = bluesky::new_key().unwrap();
    let b = bluesky::new_key().unwrap();
    assert!(bluesky::valid_key(&a) && b > a);
    for key in ["everylot-1234567890", "zzzzzzzzzzzzz", "3JZFCIJPJ2Z2A"] {
        assert!(!bluesky::valid_key(key));
    }
}
#[test]
fn addresses_and_coordinate_fallback_match_legacy() {
    let dir = fixture();
    let mut lot = database(&dir)
        .next(Platform::Bluesky, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        domain::compose(&lot, "{address} ({id})").unwrap().text,
        "2015 North Damen Avenue (1431213021)"
    );
    lot.address = "CHICAGO, IL".into();
    assert!(domain::compose(&lot, "{address}").is_err());
    lot.lat = 41.9;
    lot.lon = -87.7;
    assert_eq!(domain::compose(&lot, "{address}").unwrap().text, lot.id);
}
#[test]
fn config_rejects_invalid_and_unsafe_defaults() {
    for entries in [
        vec![("ENABLE_BLUESKY", "yes")],
        vec![("ENABLE_TWITTER", "true")],
        vec![("LEASE_SECONDS", "20")],
        vec![("STREETVIEW_PITCH", "NaN")],
        vec![("HTTP_TIMEOUT_MS", "0")],
    ] {
        assert!(
            Config::from_values(
                entries
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect()
            )
            .is_err()
        );
    }
}
#[test]
fn streetview_parameters_preserve_address_and_key_stays_out_of_errors() {
    let dir = fixture();
    let c = posting_config(&dir, "http://127.0.0.1:1");
    let lot = database(&dir)
        .next(Platform::Bluesky, None)
        .unwrap()
        .unwrap();
    let url = streetview::build_url(&c, &lot).unwrap();
    let query: HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(query["location"], lot.address);
    assert_eq!(query["radius"], "500");
    assert_eq!(query["source"], "outdoor");
    let error = http::Http::new(Duration::from_millis(10))
        .get("http://127.0.0.1:1/?key=secret-google", None)
        .unwrap_err();
    assert!(!error.to_string().contains("secret-google"));
}
#[test]
fn bounded_http_body_rejects_oversized_responses() {
    let mock = Mock::new(vec![Reply {
        path: "/",
        status: 200,
        content_type: "text/plain",
        body: "x".repeat(20),
    }]);
    let http = http::Http::new(Duration::from_secs(1));
    assert!(http::body(http.get(&mock.url, None).unwrap(), 10).is_err());
    mock.finish();
}
#[test]
fn csv_staging_preserves_posting_state_coordinates_and_nonblank_address() {
    let dir = fixture();
    let db = database(&dir);
    db.migrate().unwrap();
    import::create_staging(&db).unwrap();
    let csv = "pin,pin10,prop_address_full,prop_address_city_name,prop_address_state,prop_address_zipcode_1\n14312130200000,1431213020,,CHICAGO,IL,60601\n14312130210000,1431213021,\"100 N NEW ST\",CHICAGO,IL,60601\n14312130220000,1431213022,\"1 N \\\"TEST\\\" ST\",CHICAGO,IL,60601\n";
    import::stage_csv(&db, csv.as_bytes()).unwrap();
    import::apply_staging(&db).unwrap();
    let (address, posted): (String, String) = db
        .connection
        .query_row(
            "SELECT address,posted_bluesky FROM lots WHERE id='1431213020'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(address.starts_with("2017"));
    assert!(posted.starts_with("https:"));
}
#[test]
fn empty_or_failed_import_does_not_modify_live_lots() {
    let dir = fixture();
    let db = database(&dir);
    import::create_staging(&db).unwrap();
    assert!(import::apply_staging(&db).is_err());
    assert!(import::stage_csv(&db, b"wrong,header\n1,2\n".as_slice()).is_err());
    assert_eq!(
        db.connection
            .query_row("SELECT COUNT(*) FROM lots", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        7
    );
}
#[test]
fn persisted_session_is_atomic_private_and_legacy_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    bluesky::persist(&path, &session(100)).unwrap();
    bluesky::persist(&path, &session(200)).unwrap();
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["did"], "did:plc:test");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[test]
fn explicit_environment_file_is_loaded_without_modification() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("app.env");
    let contents = "ENABLE_BLUESKY=true\nENABLE_TWITTER=false\nPRINT_FORMAT='{address} ({id})'\n";
    fs::write(&path, contents)?;
    let cli = Cli::try_parse_from([
        "everylotbot",
        "--env-file",
        path.to_str().unwrap(),
        "post-next",
        "--dry-run",
    ])?;
    assert_eq!(cli.env_file.as_deref(), Some(path.as_path()));
    let _ = Config::load(Some(&path))?;
    assert_eq!(fs::read_to_string(&path)?, contents);
    assert!(Config::load(Some(&dir.path().join("missing.env"))).is_err());
    Ok(())
}

fn refresh_reply() -> Reply {
    reply(
        "/xrpc/com.atproto.server.refreshSession",
        200,
        session(chrono::Utc::now().timestamp() + 3600),
    )
}
fn missing_reply() -> Reply {
    reply(
        "/xrpc/com.atproto.repo.getRecord",
        400,
        json!({"error":"RecordNotFound"}),
    )
}
#[test]
fn full_post_persists_key_and_confirms_with_mock_services() -> Result<()> {
    let dir = fixture();
    database(&dir).migrate()?;
    save_session(&dir);
    // Use a fixed durable key so the server can assert the exact returned URI.
    database(&dir).begin("1431213021", Platform::Bluesky, Some("3jzfcijpj2z2a"))?;
    let mock = Mock::new(vec![
        refresh_reply(),
        missing_reply(),
        Reply {
            path: "/image",
            status: 200,
            content_type: "image/jpeg",
            body: "mock-jpeg".into(),
        },
        reply(
            "/xrpc/com.atproto.repo.uploadBlob",
            200,
            json!({"blob":{"$type":"blob","ref":{"$link":"bafk-test"},"mimeType":"image/jpeg","size":9}}),
        ),
        reply(
            "/xrpc/com.atproto.repo.createRecord",
            200,
            json!({"uri":"at://did:plc:test/app.bsky.feed.post/3jzfcijpj2z2a"}),
        ),
    ]);
    let c = posting_config(&dir, &mock.url);
    let result = app::run_with_source(
        &c,
        &c.platforms,
        None,
        false,
        Some(&format!("{}/image", mock.url)),
    )?;
    assert_eq!(result["outcome"], "posted");
    assert_eq!(
        database(&dir)
            .delivery("1431213021", Platform::Bluesky)?
            .unwrap()
            .state,
        "confirmed"
    );
    let requests = mock.finish();
    let sent: Value =
        serde_json::from_str(requests.last().unwrap().split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(sent["rkey"], "3jzfcijpj2z2a");
    assert!(requests.last().unwrap().contains("2015 North Damen Avenue"));
    Ok(())
}
#[test]
fn reconciliation_does_not_download_or_upload_image() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    db.begin("1431213021", Platform::Bluesky, Some("3jzfcijpj2z2a"))?;
    db.fail("1431213021", Platform::Bluesky, "timeout", true)?;
    save_session(&dir);
    let mock = Mock::new(vec![
        refresh_reply(),
        reply(
            "/xrpc/com.atproto.repo.getRecord",
            200,
            json!({"uri":"at://did:plc:test/app.bsky.feed.post/3jzfcijpj2z2a","value":{"text":"2015 North Damen Avenue"}}),
        ),
    ]);
    let c = posting_config(&dir, &mock.url);
    assert_eq!(
        app::run(&c, &c.platforms, None, false)?["outcome"],
        "posted"
    );
    assert_eq!(mock.finish().len(), 2);
    Ok(())
}
#[test]
fn uncertain_write_retains_key_and_reconciles_on_next_run() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    save_session(&dir);
    db.begin("1431213021", Platform::Bluesky, Some("3jzfcijpj2z2a"))?;
    let mock = Mock::new(vec![
        refresh_reply(),
        missing_reply(),
        Reply {
            path: "/image",
            status: 200,
            content_type: "image/jpeg",
            body: "image".into(),
        },
        reply(
            "/xrpc/com.atproto.repo.uploadBlob",
            200,
            json!({"blob":{"ref":{"$link":"blob"}}}),
        ),
        reply(
            "/xrpc/com.atproto.repo.createRecord",
            503,
            json!({"error":"timeout"}),
        ),
        refresh_reply(),
        reply(
            "/xrpc/com.atproto.repo.getRecord",
            200,
            json!({"value":{"text":"2015 North Damen Avenue"}}),
        ),
    ]);
    let c = posting_config(&dir, &mock.url);
    assert!(
        app::run_with_source(
            &c,
            &c.platforms,
            None,
            false,
            Some(&format!("{}/image", mock.url))
        )
        .is_err()
    );
    let delivery = db.delivery("1431213021", Platform::Bluesky)?.unwrap();
    assert_eq!(delivery.state, "unknown");
    assert_eq!(delivery.key.as_deref(), Some("3jzfcijpj2z2a"));
    assert_eq!(
        app::run(&c, &c.platforms, None, false)?["outcome"],
        "posted"
    );
    mock.finish();
    Ok(())
}
#[test]
fn lookup_failure_cannot_create_or_clear_uncertainty() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    save_session(&dir);
    db.begin("1431213021", Platform::Bluesky, Some("3jzfcijpj2z2a"))?;
    let mock = Mock::new(vec![
        refresh_reply(),
        reply(
            "/xrpc/com.atproto.repo.getRecord",
            404,
            json!({"error":"UpstreamError"}),
        ),
    ]);
    let c = posting_config(&dir, &mock.url);
    assert!(app::run(&c, &c.platforms, None, false).is_err());
    assert_eq!(
        db.delivery("1431213021", Platform::Bluesky)?.unwrap().state,
        "unknown"
    );
    mock.finish();
    Ok(())
}
#[test]
fn invalid_uncertain_key_blocks_without_network_or_replacing_key() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    db.begin("1431213021", Platform::Bluesky, Some("old-invalid"))?;
    let c = posting_config(&dir, "http://127.0.0.1:1");
    assert!(app::run(&c, &c.platforms, None, false).is_err());
    assert_eq!(
        db.delivery("1431213021", Platform::Bluesky)?
            .unwrap()
            .key
            .as_deref(),
        Some("old-invalid")
    );
    Ok(())
}
#[test]
fn refresh_rejects_changed_account_without_overwriting_session() {
    let dir = fixture();
    save_session(&dir);
    let path = dir.path().join("session.json");
    let before = fs::read(&path).unwrap();
    let mut changed = session(9999999999);
    changed["did"] = json!("did:plc:wrong");
    let mock = Mock::new(vec![reply(
        "/xrpc/com.atproto.server.refreshSession",
        200,
        changed,
    )]);
    let c = posting_config(&dir, &mock.url);
    assert!(bluesky::Bluesky::authenticate(&c, &http::Http::new(c.timeout)).is_err());
    assert_eq!(before, fs::read(path).unwrap());
    mock.finish();
}
#[test]
fn malformed_or_unknown_templates_match_legacy_brace_semantics() {
    let dir = fixture();
    let lot = database(&dir)
        .next(Platform::Bluesky, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        domain::compose(&lot, "{{id}} {} {unclosed").unwrap().text,
        "{1431213021} {} {unclosed"
    );
    assert!(domain::compose(&lot, "{unknown}").is_err());
}
#[test]
fn centroid_import_stages_latest_valid_data_and_preserves_history() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    db.connection.execute(
        "UPDATE lots SET address='' WHERE id IN ('1431213020','1431213021')",
        [],
    )?;
    let mock = Mock::new(vec![reply(
        "/centroids",
        200,
        json!([{"pin10":"1431213021","year":"2025","lat":"0","lon":"0"},{"pin10":"1431213021","year":"2024","lat":"41.9","lon":"-87.7"},{"pin10":"1431213020","year":"2025","lat":"41.8","lon":"-87.6"}]),
    )]);
    let c = config(
        &dir.path().join("lots.db"),
        &[("CHICAGO_DATA_PORTAL_TOKEN", "token")],
    );
    let result = import::enrich_from(&c, 75, &format!("{}/centroids", mock.url))?;
    assert_eq!(result["updated"], 1);
    let lat: f64 =
        db.connection
            .query_row("SELECT lat FROM lots WHERE id='1431213020'", [], |r| {
                r.get(0)
            })?;
    assert_eq!(lat, 0.0);
    mock.finish();
    Ok(())
}
#[test]
fn csv_network_error_does_not_apply_partially_staged_pages() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    let before: Vec<u8> = fs::read(dir.path().join("lots.db"))?;
    let mock=Mock::new(vec![Reply{path:"/addresses",status:200,content_type:"text/csv",body:"pin,pin10,prop_address_full,prop_address_city_name,prop_address_state,prop_address_zipcode_1\n14312130210000,1431213021,999 N CHANGED ST,CHICAGO,IL,60601\n".into()},reply("/addresses",503,json!({}))]);
    let c = config(
        &dir.path().join("lots.db"),
        &[("CHICAGO_DATA_PORTAL_TOKEN", "token")],
    );
    assert!(
        import::ingest_from(&c, "2023", "CHICAGO", 1, &format!("{}/addresses", mock.url)).is_err()
    );
    assert_eq!(before, fs::read(dir.path().join("lots.db"))?);
    mock.finish();
    Ok(())
}

#[test]
fn first_twitter_start_is_durable_even_when_first_attempt_fails() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    let c = config(
        &dir.path().join("lots.db"),
        &[
            ("ENABLE_BLUESKY", "false"),
            ("ENABLE_TWITTER", "true"),
            ("TWITTER_START_PIN10", "1431213019"),
            ("GOOGLE_API_KEY", "unused"),
            ("TWITTER_CONSUMER_KEY", "unused"),
            ("TWITTER_CONSUMER_SECRET", "unused"),
            ("TWITTER_ACCESS_TOKEN", "unused"),
            ("TWITTER_ACCESS_TOKEN_SECRET", "unused"),
        ],
    );
    db.begin("1431213020", Platform::Twitter, None)?;
    assert!(app::run(&c, &c.platforms, None, false).is_err());
    assert_eq!(
        db.high_water(Platform::Twitter, Some("0000000000"))?
            .as_deref(),
        Some("1431213019")
    );
    Ok(())
}

#[test]
fn refresh_without_did_document_drops_stale_pds_route() -> Result<()> {
    let dir = fixture();
    let mut stored = session(0);
    stored["didDoc"] = json!({"service":[{"id":"#atproto_pds","type":"AtprotoPersonalDataServer","serviceEndpoint":"https://stale.invalid"}]});
    bluesky::persist(&dir.path().join("session.json"), &stored)?;
    let mock = Mock::new(vec![refresh_reply(), missing_reply()]);
    let c = posting_config(&dir, &mock.url);
    let client = bluesky::Bluesky::authenticate(&c, &http::Http::new(c.timeout))?;
    assert!(
        client
            .reconcile(
                "3jzfcijpj2z2a",
                &domain::Post {
                    text: "text".into(),
                    alt: "alt".into()
                }
            )?
            .is_none()
    );
    let stored: Value = serde_json::from_slice(&fs::read(dir.path().join("session.json"))?)?;
    assert!(stored.get("didDoc").is_none());
    mock.finish();
    Ok(())
}

#[test]
fn oauth_signature_matches_legacy_percent_encoding() {
    let dir = fixture();
    let c = config(
        &dir.path().join("lots.db"),
        &[
            ("TWITTER_CONSUMER_KEY", "key+&"),
            ("TWITTER_CONSUMER_SECRET", "secret!"),
            ("TWITTER_ACCESS_TOKEN", "token/"),
            ("TWITTER_ACCESS_TOKEN_SECRET", "access*"),
        ],
    );
    let header = twitter::authorization(
        &c,
        "https://api.x.com/2/tweets",
        "fixed-nonce",
        "1700000000",
    )
    .unwrap();
    // Expected signature computed with the previous TypeScript implementation.
    assert!(header.contains("oauth_signature=\"4p%2BdnTgojD3vacAQzU2lL50JXgE%3D\""));
}

#[test]
fn response_body_timeout_is_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0u8; 4096];
        assert!(stream.read(&mut buffer).unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n")
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        let _ = stream.write_all(b"slow");
    });
    let http = http::Http::new(Duration::from_millis(50));
    let result = http
        .get(&url, None)
        .and_then(|response| http::body(response, 10));
    assert!(result.is_err());
    server.join().unwrap();
}

#[test]
fn conflicting_remote_record_never_advances_cursor() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    db.migrate()?;
    save_session(&dir);
    db.begin("1431213021", Platform::Bluesky, Some("3jzfcijpj2z2a"))?;
    let mock = Mock::new(vec![
        refresh_reply(),
        reply(
            "/xrpc/com.atproto.repo.getRecord",
            200,
            json!({"value":{"text":"unrelated"}}),
        ),
    ]);
    let c = posting_config(&dir, &mock.url);
    assert!(app::run(&c, &c.platforms, None, false).is_err());
    assert_eq!(db.next(Platform::Bluesky, None)?.unwrap().id, "1431213021");
    mock.finish();
    Ok(())
}

#[test]
fn csv_handles_bom_quoted_commas_newlines_and_duplicate_pin10() -> Result<()> {
    let dir = fixture();
    let db = database(&dir);
    import::create_staging(&db)?;
    let source = "\u{feff}pin,pin10,prop_address_full,prop_address_city_name,prop_address_state,prop_address_zipcode_1\r\n14312130210000,1431213021,\"100 N \\\"TEST\\\" ST\",CHICAGO,IL,60601\r\n";
    // Separate fixture for RFC4180 quoting, including a newline inside a field.
    let source = source.replace("\\\"", "\"\"")
        + "14312130210001,1431213021,\"200 N TEST,\nST\",CHICAGO,IL,60601\n14312130210002,1431213021,,CHICAGO,IL,60601\n";
    assert_eq!(import::stage_csv(&db, source.as_bytes())?, (3, 3));
    import::apply_staging(&db)?;
    let address: String =
        db.connection
            .query_row("SELECT address FROM lots WHERE id='1431213021'", [], |r| {
                r.get(0)
            })?;
    assert_eq!(address, "200 N TEST,\nST, CHICAGO, IL 60601");
    Ok(())
}
