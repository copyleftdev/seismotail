//! Earthquake Early Warning (EEW) module.
//!
//! Provides detection algorithms and integration with OpenEEW data.

use serde::{Deserialize, Serialize};

// ============================================================================
// OpenEEW Data Structures
// ============================================================================

/// OpenEEW accelerometer record from AWS public dataset.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccelerometerRecord {
    /// Device ID (e.g., "mx-001")
    pub device_id: String,
    /// Timestamp (Unix epoch in seconds)
    #[serde(rename = "cloud_t")]
    pub timestamp: f64,
    /// X-axis acceleration samples (cm/s²)
    pub x: Vec<f32>,
    /// Y-axis acceleration samples (cm/s²)
    pub y: Vec<f32>,
    /// Z-axis acceleration samples (cm/s²)
    pub z: Vec<f32>,
    /// Sample rate (typically 31.25 or 125 Hz)
    #[serde(default = "default_sample_rate")]
    pub sr: f32,
}

fn default_sample_rate() -> f32 {
    31.25
}

/// Detection result from STA/LTA algorithm.
#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    /// Device that triggered
    pub device_id: String,
    /// Timestamp of detection
    pub timestamp: f64,
    /// Peak Ground Acceleration in gals (cm/s²)
    pub pga: f32,
    /// STA/LTA ratio at trigger
    pub sta_lta_ratio: f32,
    /// Estimated magnitude (if available)
    pub estimated_magnitude: Option<f32>,
    /// Alert level
    pub alert_level: AlertLevel,
}

/// Alert severity levels based on PGA.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum AlertLevel {
    /// < 1 gal - Not felt
    None,
    /// 1-3 gals - Weak, may be felt
    Weak,
    /// 3-10 gals - Light shaking
    Light,
    /// 10-50 gals - Moderate, potential damage
    Moderate,
    /// 50-150 gals - Strong, likely damage
    Strong,
    /// > 150 gals - Severe, major damage
    Severe,
}

impl AlertLevel {
    pub fn from_pga(pga: f32) -> Self {
        match pga {
            p if p >= 150.0 => AlertLevel::Severe,
            p if p >= 50.0 => AlertLevel::Strong,
            p if p >= 10.0 => AlertLevel::Moderate,
            p if p >= 3.0 => AlertLevel::Light,
            p if p >= 1.0 => AlertLevel::Weak,
            _ => AlertLevel::None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AlertLevel::None => "none",
            AlertLevel::Weak => "weak",
            AlertLevel::Light => "light",
            AlertLevel::Moderate => "moderate",
            AlertLevel::Strong => "strong",
            AlertLevel::Severe => "severe",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            AlertLevel::None => "⚪",
            AlertLevel::Weak => "🟢",
            AlertLevel::Light => "🟡",
            AlertLevel::Moderate => "🟠",
            AlertLevel::Strong => "🔴",
            AlertLevel::Severe => "🟣",
        }
    }
}

// ============================================================================
// STA/LTA Detection Algorithm
// ============================================================================

/// STA/LTA (Short-Term Average / Long-Term Average) detector.
///
/// This is the industry-standard algorithm for P-wave detection in EEW systems.
/// When the ratio of short-term energy to long-term energy exceeds a threshold,
/// it indicates the arrival of seismic waves.
#[derive(Debug, Clone)]
pub struct StaLtaDetector {
    /// Short-term window length in samples
    sta_samples: usize,
    /// Long-term window length in samples
    lta_samples: usize,
    /// Trigger threshold (typically 2.5-4.0)
    trigger_threshold: f32,
    /// Detrigger threshold (typically 1.0-2.0)
    detrigger_threshold: f32,
}

impl Default for StaLtaDetector {
    fn default() -> Self {
        Self {
            sta_samples: 10,       // ~0.3 seconds at 31.25 Hz
            lta_samples: 100,      // ~3 seconds at 31.25 Hz
            trigger_threshold: 3.0,
            detrigger_threshold: 1.5,
        }
    }
}

impl StaLtaDetector {
    /// Create a new detector with custom parameters.
    pub fn new(sta_seconds: f32, lta_seconds: f32, sample_rate: f32, threshold: f32) -> Self {
        Self {
            sta_samples: (sta_seconds * sample_rate) as usize,
            lta_samples: (lta_seconds * sample_rate) as usize,
            trigger_threshold: threshold,
            detrigger_threshold: threshold / 2.0,
        }
    }

    /// Calculate the vector magnitude (PGA) from x, y, z components.
    #[inline]
    pub fn calculate_pga(x: f32, y: f32, z: f32) -> f32 {
        (x * x + y * y + z * z).sqrt()
    }

    /// Run detection on accelerometer data.
    ///
    /// Returns a vector of (sample_index, sta_lta_ratio, pga) for each trigger.
    pub fn detect(&self, record: &AccelerometerRecord) -> Vec<Detection> {
        let n = record.x.len().min(record.y.len()).min(record.z.len());
        if n < self.lta_samples {
            return vec![];
        }

        // Calculate PGA for each sample
        let pga: Vec<f32> = (0..n)
            .map(|i| Self::calculate_pga(record.x[i], record.y[i], record.z[i]))
            .collect();

        let mut detections = vec![];
        let mut triggered = false;

        for i in self.lta_samples..n {
            // Calculate STA (short-term average)
            let sta: f32 = pga[i.saturating_sub(self.sta_samples)..i]
                .iter()
                .sum::<f32>()
                / self.sta_samples as f32;

            // Calculate LTA (long-term average)
            let lta: f32 = pga[i.saturating_sub(self.lta_samples)..i]
                .iter()
                .sum::<f32>()
                / self.lta_samples as f32;

            // Avoid division by zero
            if lta < 0.001 {
                continue;
            }

            let ratio = sta / lta;

            if !triggered && ratio > self.trigger_threshold {
                triggered = true;
                let peak_pga = pga[i.saturating_sub(self.sta_samples)..i]
                    .iter()
                    .cloned()
                    .fold(0.0f32, f32::max);

                detections.push(Detection {
                    device_id: record.device_id.clone(),
                    timestamp: record.timestamp + (i as f64 / record.sr as f64),
                    pga: peak_pga,
                    sta_lta_ratio: ratio,
                    estimated_magnitude: estimate_magnitude_from_pga(peak_pga),
                    alert_level: AlertLevel::from_pga(peak_pga),
                });
            } else if triggered && ratio < self.detrigger_threshold {
                triggered = false;
            }
        }

        detections
    }
}

/// Estimate magnitude from Peak Ground Acceleration.
///
/// Uses simplified Gutenberg-Richter relationship.
/// This is a rough estimate - real systems use more sophisticated methods.
fn estimate_magnitude_from_pga(pga: f32) -> Option<f32> {
    if pga < 0.1 {
        return None;
    }
    // Simplified: M ≈ log10(PGA) + 2.5
    // Real systems use distance, depth, and more
    Some((pga.log10() + 2.5).clamp(1.0, 9.0))
}

// ============================================================================
// OpenEEW AWS Data Client
// ============================================================================

/// AWS S3 bucket for OpenEEW public data.
pub const OPENEEW_BUCKET: &str = "grillo-openeew";

/// Countries with available data.
#[derive(Debug, Clone, Copy)]
pub enum Country {
    Mexico,
    Chile,
}

impl Country {
    pub fn code(&self) -> &'static str {
        match self {
            Country::Mexico => "mx",
            Country::Chile => "cl",
        }
    }
}

/// Build the S3 URL for OpenEEW data.
pub fn build_s3_url(country: Country, date: &str, hour: &str) -> String {
    // Format: s3://grillo-openeew/records/country_code=mx/year=2018/month=02/day=16/hour=23/
    format!(
        "https://{}.s3.amazonaws.com/records/country_code={}/year={}/month={}/day={}/hour={}/",
        OPENEEW_BUCKET,
        country.code(),
        &date[0..4],   // year
        &date[5..7],   // month
        &date[8..10],  // day
        hour
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pga_calculation() {
        // 3-4-5 triangle: sqrt(9 + 16 + 0) = 5
        assert!((StaLtaDetector::calculate_pga(3.0, 4.0, 0.0) - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_alert_level() {
        assert_eq!(AlertLevel::from_pga(0.5), AlertLevel::None);
        assert_eq!(AlertLevel::from_pga(2.0), AlertLevel::Weak);
        assert_eq!(AlertLevel::from_pga(5.0), AlertLevel::Light);
        assert_eq!(AlertLevel::from_pga(25.0), AlertLevel::Moderate);
        assert_eq!(AlertLevel::from_pga(100.0), AlertLevel::Strong);
        assert_eq!(AlertLevel::from_pga(200.0), AlertLevel::Severe);
    }

    #[test]
    fn test_sta_lta_no_earthquake() {
        // Simulate quiet background noise (~0.1 gal)
        let record = AccelerometerRecord {
            device_id: "test-001".to_string(),
            timestamp: 1000.0,
            x: vec![0.1; 200],
            y: vec![0.1; 200],
            z: vec![0.1; 200],
            sr: 31.25,
        };

        let detector = StaLtaDetector::default();
        let detections = detector.detect(&record);
        assert!(detections.is_empty(), "Should not detect earthquake in quiet data");
    }

    #[test]
    fn test_sta_lta_earthquake() {
        // Simulate quiet then sudden spike (earthquake P-wave)
        let mut x = vec![0.1; 150];
        x.extend(vec![10.0; 50]); // Sudden spike to 10 gals
        
        let record = AccelerometerRecord {
            device_id: "test-001".to_string(),
            timestamp: 1000.0,
            x: x.clone(),
            y: x.clone(),
            z: vec![0.1; 200],
            sr: 31.25,
        };

        let detector = StaLtaDetector::default();
        let detections = detector.detect(&record);
        assert!(!detections.is_empty(), "Should detect earthquake spike");
        assert!(detections[0].pga > 10.0, "PGA should be high");
    }
}
