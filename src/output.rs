//! Output formatters for earthquake events.
//!
//! Supports human-readable (with colors), JSON, and NDJSON formats.

use std::io::{self, Write};

use crate::models::{Feature, OutputEvent};

// ANSI color codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

// Magnitude-based colors (RFC 003)
const RED: &str = "\x1b[91m";      // Critical: mag >= 7.0
const YELLOW: &str = "\x1b[93m";   // Warning: mag >= 6.0
const CYAN: &str = "\x1b[96m";     // Significant: mag >= 4.5
const GREEN: &str = "\x1b[92m";    // Moderate: mag >= 3.0
const WHITE: &str = "\x1b[97m";    // Minor: mag < 3.0

// Alert level colors
const ALERT_GREEN: &str = "\x1b[42;30m";   // Green background
const ALERT_YELLOW: &str = "\x1b[43;30m";  // Yellow background
const ALERT_ORANGE: &str = "\x1b[48;5;208;30m"; // Orange background
const ALERT_RED: &str = "\x1b[41;97m";     // Red background

// Icons for visual richness
const ICON_QUAKE: &str = "🌍";
const ICON_TSUNAMI: &str = "🌊";
const ICON_ALERT: &str = "⚠️";

/// Output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Human-readable terminal output (default)
    #[default]
    Human,
    /// JSON array
    Json,
    /// Newline-delimited JSON (one object per line)
    Ndjson,
}

impl std::str::FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            "ndjson" => Ok(Self::Ndjson),
            _ => Err(format!("unknown format: {s} (expected: human, json, ndjson)")),
        }
    }
}

/// Get the color code for a magnitude value.
fn magnitude_color(mag: Option<f64>) -> &'static str {
    match mag {
        Some(m) if m >= 7.0 => RED,
        Some(m) if m >= 6.0 => YELLOW,
        Some(m) if m >= 4.5 => CYAN,
        Some(m) if m >= 3.0 => GREEN,
        _ => WHITE,
    }
}

/// Get severity label for magnitude.
fn magnitude_label(mag: Option<f64>) -> &'static str {
    match mag {
        Some(m) if m >= 7.0 => "MAJOR",
        Some(m) if m >= 6.0 => "STRONG",
        Some(m) if m >= 4.5 => "MODERATE",
        Some(m) if m >= 3.0 => "LIGHT",
        Some(m) if m >= 2.0 => "MINOR",
        _ => "MICRO",
    }
}

/// Format alert level with color.
fn format_alert(alert: Option<&str>) -> String {
    match alert {
        Some("red") => format!(" {ALERT_RED} RED {RESET}"),
        Some("orange") => format!(" {ALERT_ORANGE} ORANGE {RESET}"),
        Some("yellow") => format!(" {ALERT_YELLOW} YELLOW {RESET}"),
        Some("green") => format!(" {ALERT_GREEN} GREEN {RESET}"),
        _ => String::new(),
    }
}

/// Write events in human-readable format with rich colors.
///
/// Format: Rich, color-coded output by magnitude
///
/// # Errors
///
/// Returns an error if writing fails.
pub fn write_human<W: Write>(writer: &mut W, events: &[Feature]) -> io::Result<()> {
    for event in events {
        let time = event
            .time()
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "unknown".into());

        let mag = event.properties.mag;
        let mag_str = mag
            .map(|m| format!("{m:.1}"))
            .unwrap_or_else(|| "?".into());

        let mag_type = event
            .properties
            .mag_type
            .as_deref()
            .unwrap_or("?");

        let depth = event.depth_km();
        let place = event
            .properties
            .place
            .as_deref()
            .unwrap_or("Unknown location");

        let color = magnitude_color(mag);
        let label = magnitude_label(mag);
        let alert = format_alert(event.properties.alert.as_deref());
        
        // Tsunami warning indicator
        let tsunami = if event.properties.tsunami != 0 {
            format!(" {ICON_TSUNAMI}")
        } else {
            String::new()
        };

        // Alert indicator
        let alert_icon = if event.properties.alert.is_some() {
            format!(" {ICON_ALERT}")
        } else {
            String::new()
        };

        writeln!(
            writer,
            "{ICON_QUAKE} {color}{BOLD}M{mag_str}{RESET} {DIM}{mag_type}{RESET} │ \
             {color}{label:8}{RESET} │ \
             {DIM}{depth:>5.0}km{RESET} │ \
             {time} UTC │ \
             {place}{tsunami}{alert_icon}{alert}"
        )?;
    }
    Ok(())
}

/// Write events as a JSON array.
///
/// # Errors
///
/// Returns an error if serialization or writing fails.
pub fn write_json<W: Write>(writer: &mut W, events: &[Feature]) -> io::Result<()> {
    let output: Vec<OutputEvent> = events.iter().map(OutputEvent::from).collect();
    let json = serde_json::to_string_pretty(&output)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(writer, "{json}")
}

/// Write events as newline-delimited JSON.
///
/// Each event is written as a single line of JSON.
///
/// # Errors
///
/// Returns an error if serialization or writing fails.
pub fn write_ndjson<W: Write>(writer: &mut W, events: &[Feature]) -> io::Result<()> {
    for event in events {
        let output = OutputEvent::from(event);
        let json = serde_json::to_string(&output)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        writeln!(writer, "{json}")?;
    }
    Ok(())
}

/// Write events in the specified format.
///
/// # Errors
///
/// Returns an error if writing fails.
pub fn write_events<W: Write>(writer: &mut W, events: &[Feature], format: Format) -> io::Result<()> {
    match format {
        Format::Human => write_human(writer, events),
        Format::Json => write_json(writer, events),
        Format::Ndjson => write_ndjson(writer, events),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_parse() {
        assert_eq!("human".parse::<Format>().unwrap(), Format::Human);
        assert_eq!("json".parse::<Format>().unwrap(), Format::Json);
        assert_eq!("ndjson".parse::<Format>().unwrap(), Format::Ndjson);
        assert!("invalid".parse::<Format>().is_err());
    }
}
