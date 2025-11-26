//! SeismoTail - Real-time earthquake monitoring from your terminal.
//!
//! A terminal-first, pipe-friendly, Prometheus-native CLI for streaming
//! and querying earthquake data from the USGS.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::error;

mod cli;
mod client;
mod dedup;
mod errors;
mod filters;
mod models;
mod output;
mod server;

use cli::{Cli, Command};
use client::UsgsClient;
use filters::EventFilter;
use models::Feature;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e:#}");
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing based on verbosity
    init_tracing(cli.verbose, cli.quiet);

    match cli.command {
        Command::Tail(args) => cmd_tail(args),
        Command::Live(args) => cmd_live(args),
        Command::Query(args) => cmd_query(args),
        Command::Ui(args) => cmd_ui(args),
    }
}

/// Initialize tracing subscriber.
fn init_tracing(verbose: bool, quiet: bool) {
    use tracing_subscriber::EnvFilter;

    let filter = if quiet {
        EnvFilter::new("error")
    } else if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(io::stderr)
        .init();
}

/// Execute the `tail` command - one-shot fetch of recent earthquakes.
fn cmd_tail(args: cli::TailArgs) -> Result<()> {
    let client = UsgsClient::new().context("failed to create USGS client")?;

    let feed = client
        .fetch_feed(args.feed)
        .context("failed to fetch earthquake feed")?;

    // Build filter from args
    let filter = EventFilter {
        min_magnitude: args.min_magnitude,
        max_depth: args.max_depth,
        bbox: args.bbox,
        radius: args.radius,
        significant_only: args.significant,
    };

    // Filter events
    let mut events: Vec<&Feature> = feed
        .features
        .iter()
        .filter(|e| filter.matches(e))
        .collect();

    // Sort by time descending (most recent first)
    events.sort_by(|a, b| b.properties.time.cmp(&a.properties.time));

    // Limit results
    events.truncate(args.limit);

    // Convert back to owned for output
    let events: Vec<Feature> = events.into_iter().cloned().collect();

    // Write output
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    output::write_events(&mut handle, &events, args.format)?;

    Ok(())
}

/// Execute the `live` command - real-time streaming.
fn cmd_live(args: cli::LiveArgs) -> Result<()> {
    // Validate poll interval
    let poll_interval = args.poll_interval.max(30);
    if poll_interval != args.poll_interval {
        tracing::warn!("poll interval clamped to minimum of 30 seconds");
    }

    let client = UsgsClient::new().context("failed to create USGS client")?;

    // Build filter from args
    let filter = EventFilter {
        min_magnitude: args.min_magnitude,
        max_depth: args.max_depth,
        bbox: args.bbox,
        radius: args.radius,
        significant_only: args.significant,
    };

    // Bounded deduplication ring (NASA Power of 10: bounded resources)
    let mut dedup = dedup::DedupeRing::with_default_capacity();

    tracing::info!(
        "streaming earthquakes from {} feed (poll every {}s)",
        args.feed.as_str(),
        poll_interval
    );

    // Print startup banner
    {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        writeln!(handle, "\x1b[1m🌍 SeismoTail Live Stream\x1b[0m")?;
        writeln!(handle, "\x1b[2mFeed: {} | Poll: {}s | Press Ctrl+C to stop\x1b[0m", 
                 args.feed.as_str(), poll_interval)?;
        writeln!(handle, "\x1b[2m─────────────────────────────────────────────────────────────────────\x1b[0m")?;
    }

    let mut poll_count = 0u64;

    loop {
        poll_count += 1;
        
        match client.fetch_feed(args.feed) {
            Ok(feed) => {
                let stdout = io::stdout();
                let mut handle = stdout.lock();
                let mut new_count = 0u64;
                let mut update_count = 0u64;

                for event in &feed.features {
                    // Apply filters first (before dedup check)
                    if !filter.matches(event) {
                        continue;
                    }

                    // Check deduplication with update detection
                    let dedup_result = dedup.check_and_mark(&event.id, event.properties.updated);
                    
                    if !dedup_result.should_emit() {
                        continue;
                    }

                    if dedup_result.is_update() {
                        update_count += 1;
                        // Optionally show update indicator
                        write!(handle, "\x1b[2m↻ UPDATE: \x1b[0m")?;
                    } else {
                        new_count += 1;
                    }

                    // Output event
                    if let Err(e) = output::write_events(&mut handle, &[event.clone()], args.format) {
                        tracing::warn!("failed to write event: {}", e);
                    }

                    // Flush after each event for real-time output
                    let _ = handle.flush();
                }

                // Log poll stats at debug level
                if new_count > 0 || update_count > 0 {
                    tracing::debug!(
                        "poll #{}: {} new, {} updates (dedup rate: {:.1}%)",
                        poll_count,
                        new_count,
                        update_count,
                        dedup.dupe_rate() * 100.0
                    );
                }
            }
            Err(e) => {
                tracing::warn!("fetch failed, will retry: {}", e);
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(poll_interval));
    }
}

/// Execute the `query` command - historical search.
fn cmd_query(_args: cli::QueryArgs) -> Result<()> {
    // TODO: Implement FDSN query in Phase 3
    anyhow::bail!("query command not yet implemented (Phase 3)")
}

/// Execute the `ui` command - start web server.
fn cmd_ui(args: cli::UiArgs) -> Result<()> {
    // Build server config
    let config = server::ServerConfig {
        port: args.port,
        host: args.host.clone(),
        feed_type: args.feed,
        poll_interval: args.poll_interval.max(30),
        filter: EventFilter {
            min_magnitude: args.min_magnitude,
            ..Default::default()
        },
    };

    // Print startup message
    let url = format!("http://{}:{}", args.host, args.port);
    println!("\x1b[1m🌍 SeismoTail Web UI\x1b[0m");
    println!("\x1b[2m───────────────────────────────────────\x1b[0m");
    println!("  Local:   \x1b[96m{}\x1b[0m", url);
    println!("  Feed:    {}", args.feed.as_str());
    println!("  Poll:    {}s", args.poll_interval);
    println!("\x1b[2m───────────────────────────────────────\x1b[0m");
    println!("\x1b[2mPress Ctrl+C to stop\x1b[0m\n");

    // Open browser if requested (using xdg-open/open command)
    if args.open {
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd").args(["/c", "start", &url]).spawn();
    }

    // Run the async server on tokio runtime
    tokio::runtime::Runtime::new()
        .context("failed to create tokio runtime")?
        .block_on(server::run_server(config))
}
