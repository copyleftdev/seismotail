//! Command-line interface definitions.
//!
//! Uses clap derive API for argument parsing.

use clap::{Parser, Subcommand};

use crate::client::FeedType;
use crate::filters::{BBox, RadiusFilter};
use crate::output::Format;

/// Real-time earthquake monitoring from your terminal.
#[derive(Parser, Debug)]
#[command(name = "seismotail")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Command to run
    #[command(subcommand)]
    pub command: Command,

    /// Enable verbose debug logging
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Suppress all output except errors
    #[arg(long, global = true)]
    pub quiet: bool,
}

/// Available commands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show recent earthquakes (one-shot fetch and exit)
    Tail(TailArgs),

    /// Stream earthquakes in real-time
    Live(LiveArgs),

    /// Query historical earthquakes
    Query(QueryArgs),

    /// Start the web UI server
    Ui(UiArgs),
}

/// Arguments for the `tail` command.
#[derive(Parser, Debug)]
pub struct TailArgs {
    /// Feed type to fetch
    #[arg(long, default_value = "2.5_day", value_parser = parse_feed_type)]
    pub feed: FeedType,

    /// Minimum magnitude to show
    #[arg(long)]
    pub min_magnitude: Option<f64>,

    /// Maximum depth in km to show
    #[arg(long)]
    pub max_depth: Option<f64>,

    /// Bounding box filter: minlat,minlon,maxlat,maxlon
    #[arg(long, value_parser = parse_bbox)]
    pub bbox: Option<BBox>,

    /// Radius filter: lat,lon,radius_km
    #[arg(long, value_parser = parse_radius)]
    pub radius: Option<RadiusFilter>,

    /// Only show significant events (with alert level)
    #[arg(long)]
    pub significant: bool,

    /// Maximum number of events to show
    #[arg(long, short = 'n', default_value = "50")]
    pub limit: usize,

    /// Output format
    #[arg(long, short = 'f', default_value = "human", value_parser = parse_format)]
    pub format: Format,
}

/// Arguments for the `live` command.
#[derive(Parser, Debug)]
pub struct LiveArgs {
    /// Feed type to stream
    #[arg(long, default_value = "all_hour", value_parser = parse_feed_type)]
    pub feed: FeedType,

    /// Minimum magnitude to show
    #[arg(long)]
    pub min_magnitude: Option<f64>,

    /// Maximum depth in km to show
    #[arg(long)]
    pub max_depth: Option<f64>,

    /// Bounding box filter: minlat,minlon,maxlat,maxlon
    #[arg(long, value_parser = parse_bbox)]
    pub bbox: Option<BBox>,

    /// Radius filter: lat,lon,radius_km
    #[arg(long, value_parser = parse_radius)]
    pub radius: Option<RadiusFilter>,

    /// Only show significant events (with alert level)
    #[arg(long)]
    pub significant: bool,

    /// Poll interval in seconds (minimum 30)
    #[arg(long, default_value = "60")]
    pub poll_interval: u64,

    /// Output format
    #[arg(long, short = 'f', default_value = "human", value_parser = parse_format)]
    pub format: Format,
}

/// Arguments for the `query` command.
#[derive(Parser, Debug)]
pub struct QueryArgs {
    /// Start date (YYYY-MM-DD or ISO8601)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD or ISO8601, defaults to now)
    #[arg(long)]
    pub end: Option<String>,

    /// Minimum magnitude
    #[arg(long)]
    pub min_magnitude: Option<f64>,

    /// Maximum magnitude
    #[arg(long)]
    pub max_magnitude: Option<f64>,

    /// Maximum results to return
    #[arg(long, default_value = "100")]
    pub limit: usize,

    /// Output format
    #[arg(long, short = 'f', default_value = "human", value_parser = parse_format)]
    pub format: Format,
}

/// Arguments for the `ui` command.
#[derive(Parser, Debug)]
pub struct UiArgs {
    /// Port to listen on
    #[arg(long, short = 'p', default_value = "8080")]
    pub port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Feed type to stream
    #[arg(long, default_value = "all_hour", value_parser = parse_feed_type)]
    pub feed: FeedType,

    /// Poll interval in seconds
    #[arg(long, default_value = "60")]
    pub poll_interval: u64,

    /// Minimum magnitude to show
    #[arg(long)]
    pub min_magnitude: Option<f64>,

    /// Open browser automatically
    #[arg(long)]
    pub open: bool,
}

/// Parse a feed type from string.
fn parse_feed_type(s: &str) -> Result<FeedType, String> {
    s.parse()
}

/// Parse an output format from string.
fn parse_format(s: &str) -> Result<Format, String> {
    s.parse()
}

/// Parse a bounding box from string.
fn parse_bbox(s: &str) -> Result<BBox, String> {
    s.parse()
}

/// Parse a radius filter from string.
fn parse_radius(s: &str) -> Result<RadiusFilter, String> {
    s.parse()
}
