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
mod eew;
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
        Command::Detect(args) => cmd_detect(args),
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

/// Run the EEW detection demo.
fn cmd_detect(args: cli::DetectArgs) -> Result<()> {
    use crate::eew::{AccelerometerRecord, AlertLevel, StaLtaDetector};

    println!("\x1b[1m🚨 SeismoTail EEW Detection Demo\x1b[0m");
    println!("\x1b[2m───────────────────────────────────────\x1b[0m");
    println!("  Algorithm: STA/LTA (Short-Term/Long-Term Average)");
    println!("  Threshold: {}", args.threshold);
    println!("\x1b[2m───────────────────────────────────────\x1b[0m\n");

    if args.demo {
        // Demo mode: simulate earthquake detection
        println!("\x1b[93m▶ Running detection on simulated data...\x1b[0m\n");

        // Create detector with custom threshold
        let detector = StaLtaDetector::default();

        // Simulate quiet background noise (0.1 gal)
        let mut x: Vec<f32> = vec![0.1; 150];
        let mut y: Vec<f32> = vec![0.1; 150];
        let mut z: Vec<f32> = vec![0.1; 150];

        // Add P-wave arrival (sudden spike to 15 gals)
        println!("  Simulating P-wave arrival at sample 150...");
        x.extend(vec![15.0, 18.0, 22.0, 25.0, 20.0, 15.0, 10.0, 8.0, 5.0, 3.0]);
        y.extend(vec![12.0, 15.0, 18.0, 20.0, 16.0, 12.0, 8.0, 6.0, 4.0, 2.0]);
        z.extend(vec![5.0, 8.0, 10.0, 12.0, 10.0, 8.0, 5.0, 3.0, 2.0, 1.0]);

        // Add more noise after
        x.extend(vec![0.2; 40]);
        y.extend(vec![0.2; 40]);
        z.extend(vec![0.2; 40]);

        let record = AccelerometerRecord {
            device_id: "demo-sensor-001".to_string(),
            timestamp: chrono::Utc::now().timestamp() as f64,
            x,
            y,
            z,
            sr: 31.25,
        };

        let detections = detector.detect(&record);

        if detections.is_empty() {
            println!("\n  \x1b[92m✓ No earthquakes detected in quiet data\x1b[0m");
        } else {
            for det in &detections {
                let alert_color = match det.alert_level {
                    AlertLevel::Severe => "\x1b[95m",   // Magenta
                    AlertLevel::Strong => "\x1b[91m",   // Red
                    AlertLevel::Moderate => "\x1b[93m", // Yellow
                    AlertLevel::Light => "\x1b[92m",    // Green
                    _ => "\x1b[0m",
                };

                println!("\n  \x1b[1m{} EARTHQUAKE DETECTED!\x1b[0m", det.alert_level.emoji());
                println!("  ├─ Device:    {}", det.device_id);
                println!("  ├─ PGA:       {:.2} gals (cm/s²)", det.pga);
                println!("  ├─ STA/LTA:   {:.2}", det.sta_lta_ratio);
                println!("  ├─ Alert:     {}{}\x1b[0m", alert_color, det.alert_level.as_str().to_uppercase());
                if let Some(mag) = det.estimated_magnitude {
                    println!("  └─ Est. Mag:  ~M{:.1}", mag);
                }
            }
        }

        println!("\n\x1b[2m───────────────────────────────────────\x1b[0m");
        println!("\x1b[1mPGA Reference Scale:\x1b[0m");
        println!("  ⚪ < 1 gal    │ Not felt");
        println!("  🟢 1-3 gals   │ Weak");
        println!("  🟡 3-10 gals  │ Light");
        println!("  🟠 10-50 gals │ Moderate (potential damage)");
        println!("  🔴 50-150 gals│ Strong (likely damage)");
        println!("  🟣 > 150 gals │ Severe (major damage)");
        println!("\x1b[2m───────────────────────────────────────\x1b[0m\n");

        println!("\x1b[92m✓ Demo complete!\x1b[0m");
        println!("\n\x1b[2mTo analyze real OpenEEW data:\x1b[0m");
        println!("  seismotail detect --country mx --date 2018-02-16");
        println!("\n\x1b[2mData from: https://registry.opendata.aws/grillo-openeew/\x1b[0m");

    } else {
        // Real data mode
        println!("\x1b[93m▶ OpenEEW Data Access\x1b[0m\n");
        println!("  Country: {}", args.country);
        
        if let Some(date) = &args.date {
            println!("  Date: {}", date);
            println!("\n  \x1b[2mData URL pattern:\x1b[0m");
            println!("  s3://grillo-openeew/records/country_code={}/year={}/month={}/day={}/",
                args.country, &date[0..4], &date[5..7], &date[8..10]);
        }
        
        println!("\n\x1b[93m⚠ Real-time data requires AWS S3 access.\x1b[0m");
        println!("  Use --demo flag to see detection in action:");
        println!("  \x1b[96mseismotail detect --demo\x1b[0m");
    }

    Ok(())
}
