use anyhow::{Result, bail, ensure};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Lot {
    pub id: String,
    pub address: String,
    pub lat: f64,
    pub lon: f64,
    pub posted_twitter: String,
    pub posted_bluesky: String,
}
#[derive(Debug, Serialize)]
pub struct Post {
    pub text: String,
    pub alt: String,
}
pub fn usable_address(address: &str) -> bool {
    !matches!(
        address.trim().to_uppercase().as_str(),
        "" | "CHICAGO, IL" | ", CHICAGO, IL"
    )
}
pub fn coordinates(lat: f64, lon: f64) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
        && lat != 0.0
        && lon != 0.0
}
pub fn sanitize(address: &str) -> String {
    let mut result = Vec::new();
    for (i, part) in address
        .split(',')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .enumerate()
    {
        if i == 0 {
            result.push(part.to_owned());
            continue;
        }
        let direction = match part {
            "N" => Some("North"),
            "S" => Some("South"),
            "E" => Some("East"),
            "W" => Some("West"),
            _ => None,
        };
        if let Some(d) = direction {
            result.push(d.to_owned());
            continue;
        }
        let street = match part {
            "AVE" => Some("Avenue"),
            "ST" => Some("Street"),
            "BLVD" => Some("Boulevard"),
            "RD" => Some("Road"),
            "DR" => Some("Drive"),
            "CT" => Some("Court"),
            "PL" => Some("Place"),
            "TER" => Some("Terrace"),
            "LN" => Some("Lane"),
            "WAY" => Some("Way"),
            "CIR" => Some("Circle"),
            "PKY" => Some("Parkway"),
            "SQ" => Some("Square"),
            _ => None,
        };
        if let Some(s) = street {
            result.push(s.to_owned());
            break;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.push(format!(
                "{}{}",
                first.to_uppercase(),
                chars.as_str().to_lowercase()
            ));
        }
    }
    result.join(" ")
}
pub fn compose(lot: &Lot, template: &str) -> Result<Post> {
    if !usable_address(&lot.address) {
        ensure!(
            coordinates(lot.lat, lot.lon),
            "Lot {} has neither a usable address nor coordinates",
            lot.id
        );
        return Ok(Post {
            text: lot.id.clone(),
            alt: format!(
                "Google Street View near Cook County parcel PIN10 {}. This parcel does not have a common address.",
                lot.id
            ),
        });
    }
    let address = sanitize(&lot.address);
    let mut text = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        text.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('}') else {
            text.push_str(&rest[start..]);
            rest = "";
            break;
        };
        if end == 0 || tail[..end].contains('{') {
            text.push('{');
            rest = tail;
            continue;
        }
        let value = match &tail[..end] {
            "id" => lot.id.clone(),
            "address" => address.clone(),
            "lat" => lot.lat.to_string(),
            "lon" => lot.lon.to_string(),
            key => bail!("Unknown PRINT_FORMAT field: {key}"),
        };
        text.push_str(&value);
        rest = &tail[end + 1..];
    }
    text.push_str(rest);
    Ok(Post {
        text,
        alt: format!(
            "Google Streetview of the property with PIN10 {}: {address}",
            lot.id
        ),
    })
}
