use crate::{
    config::Config,
    domain::{Lot, coordinates, usable_address},
    http::{Http, body},
};
use anyhow::{Result, bail, ensure};
use std::time::Duration;
use url::Url;
pub fn build_url(config: &Config, lot: &Lot) -> Result<Url> {
    let mut url = Url::parse("https://maps.googleapis.com/maps/api/streetview")?;
    let location = if usable_address(&lot.address) {
        let trimmed = lot.address.trim();
        let parts: Vec<_> = trimmed.split(',').map(str::trim).collect();
        let chicago =
            parts.len() >= 3 && parts[parts.len() - 2].eq_ignore_ascii_case("CHICAGO") && {
                let state = parts[parts.len() - 1].to_uppercase();
                state == "IL" || state.starts_with("IL ")
            };
        if chicago {
            trimmed.to_owned()
        } else {
            format!("{trimmed}, CHICAGO, IL")
        }
    } else {
        ensure!(
            coordinates(lot.lat, lot.lon),
            "Parcel has neither address nor coordinates"
        );
        format!("{},{}", lot.lat, lot.lon)
    };
    url.query_pairs_mut()
        .append_pair("location", &location)
        .append_pair("radius", &config.radius.to_string())
        .append_pair("source", "outdoor")
        .append_pair("key", config.required("GOOGLE_API_KEY")?)
        .append_pair("size", "1000x1000")
        .append_pair("fov", "65")
        .append_pair("pitch", &config.pitch.to_string())
        .append_pair("zoom", &config.zoom.to_string())
        .append_pair("return_error_code", "true");
    Ok(url)
}
pub fn fetch(http: &Http, url: &str) -> Result<Vec<u8>> {
    for attempt in 0..3 {
        let result = (|| {
            let response = http.get(url, None)?;
            ensure!(
                response.status().is_success(),
                "Street View returned HTTP {}",
                response.status().as_u16()
            );
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            ensure!(
                content_type.to_ascii_lowercase().starts_with("image/"),
                "Street View returned an unexpected content type"
            );
            let bytes = body(response, 2_000_000)?;
            ensure!(!bytes.is_empty(), "Street View returned an empty image");
            Ok(bytes)
        })();
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(error) if attempt == 2 => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(250 << attempt)),
        }
    }
    bail!("Street View request failed")
}
