//! Event filtering logic.
//!
//! Implements geographic and attribute filters per RFC 005.

use std::f64::consts::PI;

use crate::models::Feature;

/// Earth radius in kilometers for haversine calculations.
const EARTH_RADIUS_KM: f64 = 6371.0;

/// Bounding box for geographic filtering.
#[derive(Debug, Clone, Copy)]
pub struct BBox {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

impl std::str::FromStr for BBox {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 4 {
            return Err(format!(
                "bbox requires 4 values (minlat,minlon,maxlat,maxlon), got {}",
                parts.len()
            ));
        }

        let vals: Result<Vec<f64>, _> = parts.iter().map(|p| p.trim().parse::<f64>()).collect();
        let vals = vals.map_err(|e| format!("invalid number in bbox: {e}"))?;

        let bbox = Self {
            min_lat: vals[0],
            min_lon: vals[1],
            max_lat: vals[2],
            max_lon: vals[3],
        };

        // Validate ranges
        if bbox.min_lat < -90.0 || bbox.min_lat > 90.0 {
            return Err(format!("min_lat {} out of range [-90, 90]", bbox.min_lat));
        }
        if bbox.max_lat < -90.0 || bbox.max_lat > 90.0 {
            return Err(format!("max_lat {} out of range [-90, 90]", bbox.max_lat));
        }
        if bbox.min_lon < -180.0 || bbox.min_lon > 180.0 {
            return Err(format!("min_lon {} out of range [-180, 180]", bbox.min_lon));
        }
        if bbox.max_lon < -180.0 || bbox.max_lon > 180.0 {
            return Err(format!("max_lon {} out of range [-180, 180]", bbox.max_lon));
        }
        if bbox.min_lat > bbox.max_lat {
            return Err(format!(
                "min_lat {} must be <= max_lat {}",
                bbox.min_lat, bbox.max_lat
            ));
        }

        Ok(bbox)
    }
}

impl BBox {
    /// Check if a point is within the bounding box.
    #[must_use]
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.min_lat && lat <= self.max_lat && lon >= self.min_lon && lon <= self.max_lon
    }
}

/// Radius filter for geographic filtering.
#[derive(Debug, Clone, Copy)]
pub struct RadiusFilter {
    pub center_lat: f64,
    pub center_lon: f64,
    pub radius_km: f64,
}

impl std::str::FromStr for RadiusFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 3 {
            return Err(format!(
                "radius requires 3 values (lat,lon,radius_km), got {}",
                parts.len()
            ));
        }

        let vals: Result<Vec<f64>, _> = parts.iter().map(|p| p.trim().parse::<f64>()).collect();
        let vals = vals.map_err(|e| format!("invalid number in radius: {e}"))?;

        let filter = Self {
            center_lat: vals[0],
            center_lon: vals[1],
            radius_km: vals[2],
        };

        // Validate
        if filter.center_lat < -90.0 || filter.center_lat > 90.0 {
            return Err(format!(
                "latitude {} out of range [-90, 90]",
                filter.center_lat
            ));
        }
        if filter.center_lon < -180.0 || filter.center_lon > 180.0 {
            return Err(format!(
                "longitude {} out of range [-180, 180]",
                filter.center_lon
            ));
        }
        if filter.radius_km <= 0.0 {
            return Err(format!("radius must be positive, got {}", filter.radius_km));
        }

        Ok(filter)
    }
}

impl RadiusFilter {
    /// Check if a point is within the radius using haversine formula.
    #[must_use]
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        let distance = haversine_distance(self.center_lat, self.center_lon, lat, lon);
        distance <= self.radius_km
    }
}

/// Calculate the great-circle distance between two points using the haversine formula.
///
/// Returns distance in kilometers.
#[must_use]
pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1 * PI / 180.0;
    let lat2_rad = lat2 * PI / 180.0;
    let delta_lat = (lat2 - lat1) * PI / 180.0;
    let delta_lon = (lon2 - lon1) * PI / 180.0;

    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    EARTH_RADIUS_KM * c
}

/// Combined filter criteria.
#[derive(Debug, Default, Clone)]
pub struct EventFilter {
    pub min_magnitude: Option<f64>,
    pub max_depth: Option<f64>,
    pub bbox: Option<BBox>,
    pub radius: Option<RadiusFilter>,
    pub significant_only: bool,
}

impl EventFilter {
    /// Check if an event passes all filter criteria.
    #[must_use]
    pub fn matches(&self, event: &Feature) -> bool {
        self.check_magnitude(event)
            && self.check_depth(event)
            && self.check_bbox(event)
            && self.check_radius(event)
            && self.check_significant(event)
    }

    fn check_magnitude(&self, event: &Feature) -> bool {
        match self.min_magnitude {
            None => true,
            Some(min) => event.properties.mag.map_or(false, |m| m >= min),
        }
    }

    fn check_depth(&self, event: &Feature) -> bool {
        match self.max_depth {
            None => true,
            Some(max) => event.depth_km() <= max,
        }
    }

    fn check_bbox(&self, event: &Feature) -> bool {
        match &self.bbox {
            None => true,
            Some(bbox) => bbox.contains(event.latitude(), event.longitude()),
        }
    }

    fn check_radius(&self, event: &Feature) -> bool {
        match &self.radius {
            None => true,
            Some(radius) => radius.contains(event.latitude(), event.longitude()),
        }
    }

    fn check_significant(&self, event: &Feature) -> bool {
        if !self.significant_only {
            return true;
        }
        // Significant = has an alert level set
        event.properties.alert.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbox_parse() {
        let bbox: BBox = "32.5,-124.5,42.0,-114.0".parse().unwrap();
        assert!((bbox.min_lat - 32.5).abs() < 0.001);
        assert!((bbox.min_lon - (-124.5)).abs() < 0.001);
    }

    #[test]
    fn test_bbox_contains() {
        let bbox: BBox = "32.5,-124.5,42.0,-114.0".parse().unwrap();
        assert!(bbox.contains(37.0, -120.0)); // Inside (California)
        assert!(!bbox.contains(50.0, -120.0)); // North of box
    }

    #[test]
    fn test_radius_parse() {
        let radius: RadiusFilter = "37.77,-122.41,500".parse().unwrap();
        assert!((radius.center_lat - 37.77).abs() < 0.001);
        assert!((radius.radius_km - 500.0).abs() < 0.001);
    }

    #[test]
    fn test_haversine() {
        // SF to LA is roughly 560 km
        let distance = haversine_distance(37.77, -122.41, 34.05, -118.24);
        assert!(distance > 500.0 && distance < 620.0);
    }

    #[test]
    fn test_radius_contains() {
        let radius: RadiusFilter = "37.77,-122.41,100".parse().unwrap();
        // SF to Oakland is ~15km
        assert!(radius.contains(37.80, -122.27));
        // SF to LA is ~560km
        assert!(!radius.contains(34.05, -118.24));
    }
}
