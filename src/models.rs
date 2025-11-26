//! Data models for USGS earthquake API responses.
//!
//! These structures match the GeoJSON format from USGS feeds.
//! See RFC 002 for full contract details.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::errors::SeismotailError;

/// Top-level GeoJSON response from USGS feeds.
#[derive(Debug, Clone, Deserialize)]
pub struct FeatureCollection {
    /// Always "FeatureCollection"
    #[serde(rename = "type")]
    pub type_: String,

    /// Feed metadata
    pub metadata: Metadata,

    /// Earthquake events
    pub features: Vec<Feature>,
}

impl FeatureCollection {
    /// Validate the response structure.
    pub fn validate(&self) -> Result<(), SeismotailError> {
        if self.type_ != "FeatureCollection" {
            return Err(SeismotailError::InvalidResponse(format!(
                "expected type 'FeatureCollection', got '{}'",
                self.type_
            )));
        }
        Ok(())
    }
}

/// Metadata about the feed response.
#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    /// When this feed was generated (ms since epoch)
    pub generated: i64,

    /// Feed URL
    pub url: String,

    /// Human-readable title
    pub title: String,

    /// HTTP status code
    pub status: u16,

    /// API version string
    pub api: String,

    /// Number of events in response
    pub count: usize,
}

/// A single earthquake event.
#[derive(Debug, Clone, Deserialize)]
pub struct Feature {
    /// Always "Feature"
    #[serde(rename = "type")]
    pub type_: String,

    /// Unique event ID (stable dedupe key)
    pub id: String,

    /// Geographic location
    pub geometry: Geometry,

    /// Event properties
    pub properties: Properties,
}

impl Feature {
    /// Validate the event structure.
    pub fn validate(&self) -> Result<(), SeismotailError> {
        if self.id.is_empty() {
            return Err(SeismotailError::Validation("empty event ID".into()));
        }
        if self.geometry.coordinates.len() != 3 {
            return Err(SeismotailError::Validation(format!(
                "expected 3 coordinates, got {}",
                self.geometry.coordinates.len()
            )));
        }
        Ok(())
    }

    /// Get the event time as a `DateTime<Utc>`.
    #[must_use]
    pub fn time(&self) -> Option<DateTime<Utc>> {
        Utc.timestamp_millis_opt(self.properties.time).single()
    }

    /// Get longitude (degrees).
    #[must_use]
    pub fn longitude(&self) -> f64 {
        self.geometry.coordinates.first().copied().unwrap_or(0.0)
    }

    /// Get latitude (degrees).
    #[must_use]
    pub fn latitude(&self) -> f64 {
        self.geometry.coordinates.get(1).copied().unwrap_or(0.0)
    }

    /// Get depth in kilometers (positive down).
    #[must_use]
    pub fn depth_km(&self) -> f64 {
        self.geometry.coordinates.get(2).copied().unwrap_or(0.0)
    }
}

/// Geographic geometry for an event.
#[derive(Debug, Clone, Deserialize)]
pub struct Geometry {
    /// Always "Point"
    #[serde(rename = "type")]
    pub type_: String,

    /// Coordinates: [longitude, latitude, depth_km]
    pub coordinates: Vec<f64>,
}

/// Event properties from USGS API.
#[derive(Debug, Clone, Deserialize)]
pub struct Properties {
    /// Magnitude value
    pub mag: Option<f64>,

    /// Magnitude type (mb, Ml, Mw, etc.)
    #[serde(rename = "magType")]
    pub mag_type: Option<String>,

    /// Human-readable place description
    pub place: Option<String>,

    /// Event time (ms since epoch)
    pub time: i64,

    /// Last update time (ms since epoch)
    pub updated: i64,

    /// Event status: "automatic" or "reviewed"
    pub status: String,

    /// Alert level: null, "green", "yellow", "orange", "red"
    pub alert: Option<String>,

    /// Tsunami flag: 0 or 1
    pub tsunami: i32,

    /// Significance score (0-1000+)
    pub sig: i32,

    /// Network code
    pub net: String,

    /// Event code
    pub code: String,

    /// Comma-separated event IDs
    pub ids: Option<String>,

    /// Comma-separated source networks
    pub sources: Option<String>,

    /// Available product types
    pub types: Option<String>,

    /// Number of stations used
    pub nst: Option<i32>,

    /// Distance to nearest station (degrees)
    pub dmin: Option<f64>,

    /// RMS travel time residual
    pub rms: Option<f64>,

    /// Azimuthal gap (degrees)
    pub gap: Option<f64>,

    /// Event page URL
    pub url: Option<String>,

    /// Detail GeoJSON URL
    pub detail: Option<String>,

    /// Human-readable title
    pub title: Option<String>,

    /// Number of "Did You Feel It?" reports
    pub felt: Option<i32>,

    /// Community Decimal Intensity
    pub cdi: Option<f64>,

    /// Modified Mercalli Intensity
    pub mmi: Option<f64>,

    /// Event type (earthquake, quarry, etc.)
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

/// Simplified event for output.
///
/// This is the normalized structure we emit in JSON/NDJSON output.
#[derive(Debug, Clone, Serialize)]
pub struct OutputEvent {
    pub id: String,
    pub time: String,
    pub magnitude: Option<f64>,
    pub magnitude_type: Option<String>,
    pub depth_km: f64,
    pub latitude: f64,
    pub longitude: f64,
    pub place: Option<String>,
    pub alert: Option<String>,
    pub tsunami: bool,
    pub status: String,
    pub significance: i32,
    pub url: Option<String>,
}

impl From<&Feature> for OutputEvent {
    fn from(f: &Feature) -> Self {
        Self {
            id: f.id.clone(),
            time: f
                .time()
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "unknown".into()),
            magnitude: f.properties.mag,
            magnitude_type: f.properties.mag_type.clone(),
            depth_km: f.depth_km(),
            latitude: f.latitude(),
            longitude: f.longitude(),
            place: f.properties.place.clone(),
            alert: f.properties.alert.clone(),
            tsunami: f.properties.tsunami != 0,
            status: f.properties.status.clone(),
            significance: f.properties.sig,
            url: f.properties.url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sample_feed() {
        let json = include_str!("../tools/sample_2.5_day.json");
        let feed: FeatureCollection =
            serde_json::from_str(json).expect("failed to parse sample feed");

        feed.validate().expect("invalid feed");
        assert_eq!(feed.type_, "FeatureCollection");
        assert!(!feed.features.is_empty());

        for feature in &feed.features {
            feature.validate().expect("invalid feature");
            assert!(!feature.id.is_empty());
        }
    }
}
