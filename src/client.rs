//! USGS Earthquake API client.
//!
//! Provides blocking HTTP access to USGS earthquake feeds.
//! Uses reqwest with rustls for TLS.

use std::time::Duration;

use reqwest::blocking::Client;
use tracing::{debug, instrument};

use crate::errors::SeismotailError;
use crate::models::FeatureCollection;

/// Default request timeout in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 10;

/// User agent string for API requests.
const USER_AGENT: &str = concat!("seismotail/", env!("CARGO_PKG_VERSION"));

/// USGS base URL for earthquake feeds.
const USGS_BASE_URL: &str = "https://earthquake.usgs.gov";

/// Available feed types for summary feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedType {
    AllHour,
    AllDay,
    AllWeek,
    AllMonth,
    Mag1Hour,
    Mag1Day,
    Mag1Week,
    Mag1Month,
    Mag25Hour,
    Mag25Day,
    Mag25Week,
    Mag25Month,
    Mag45Hour,
    Mag45Day,
    Mag45Week,
    Mag45Month,
    SignificantHour,
    SignificantDay,
    SignificantWeek,
    SignificantMonth,
}

impl FeedType {
    /// Get the URL path segment for this feed type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AllHour => "all_hour",
            Self::AllDay => "all_day",
            Self::AllWeek => "all_week",
            Self::AllMonth => "all_month",
            Self::Mag1Hour => "1.0_hour",
            Self::Mag1Day => "1.0_day",
            Self::Mag1Week => "1.0_week",
            Self::Mag1Month => "1.0_month",
            Self::Mag25Hour => "2.5_hour",
            Self::Mag25Day => "2.5_day",
            Self::Mag25Week => "2.5_week",
            Self::Mag25Month => "2.5_month",
            Self::Mag45Hour => "4.5_hour",
            Self::Mag45Day => "4.5_day",
            Self::Mag45Week => "4.5_week",
            Self::Mag45Month => "4.5_month",
            Self::SignificantHour => "significant_hour",
            Self::SignificantDay => "significant_day",
            Self::SignificantWeek => "significant_week",
            Self::SignificantMonth => "significant_month",
        }
    }
}

impl std::str::FromStr for FeedType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "all_hour" => Ok(Self::AllHour),
            "all_day" => Ok(Self::AllDay),
            "all_week" => Ok(Self::AllWeek),
            "all_month" => Ok(Self::AllMonth),
            "1.0_hour" => Ok(Self::Mag1Hour),
            "1.0_day" => Ok(Self::Mag1Day),
            "1.0_week" => Ok(Self::Mag1Week),
            "1.0_month" => Ok(Self::Mag1Month),
            "2.5_hour" => Ok(Self::Mag25Hour),
            "2.5_day" => Ok(Self::Mag25Day),
            "2.5_week" => Ok(Self::Mag25Week),
            "2.5_month" => Ok(Self::Mag25Month),
            "4.5_hour" => Ok(Self::Mag45Hour),
            "4.5_day" => Ok(Self::Mag45Day),
            "4.5_week" => Ok(Self::Mag45Week),
            "4.5_month" => Ok(Self::Mag45Month),
            "significant_hour" => Ok(Self::SignificantHour),
            "significant_day" => Ok(Self::SignificantDay),
            "significant_week" => Ok(Self::SignificantWeek),
            "significant_month" => Ok(Self::SignificantMonth),
            _ => Err(format!("unknown feed type: {s}")),
        }
    }
}

/// Client for USGS earthquake API.
pub struct UsgsClient {
    client: Client,
    base_url: String,
}

impl UsgsClient {
    /// Create a new USGS client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be initialized.
    pub fn new() -> Result<Self, SeismotailError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent(USER_AGENT)
            .build()?;

        Ok(Self {
            client,
            base_url: USGS_BASE_URL.to_string(),
        })
    }

    /// Fetch a summary GeoJSON feed.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed.
    #[instrument(skip(self), fields(feed = feed_type.as_str()))]
    pub fn fetch_feed(&self, feed_type: FeedType) -> Result<FeatureCollection, SeismotailError> {
        let url = format!(
            "{}/earthquakes/feed/v1.0/summary/{}.geojson",
            self.base_url,
            feed_type.as_str()
        );

        debug!("fetching feed from {}", url);

        let response = self.client.get(&url).send()?;

        // Check status before parsing
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(SeismotailError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let feed: FeatureCollection = response.json()?;

        // Validate response structure
        feed.validate()?;

        debug!("fetched {} events", feed.features.len());
        Ok(feed)
    }
}

impl Default for UsgsClient {
    fn default() -> Self {
        Self::new().expect("failed to create default UsgsClient")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_type_round_trip() {
        let types = [
            FeedType::AllHour,
            FeedType::Mag25Day,
            FeedType::SignificantWeek,
        ];

        for feed_type in types {
            let s = feed_type.as_str();
            let parsed: FeedType = s.parse().expect("failed to parse");
            assert_eq!(parsed, feed_type);
        }
    }
}
