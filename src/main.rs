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
    use crate::eew::{AccelerometerRecord, AlertLevel, Detection, OpenEewClient, StaLtaDetector};

    println!("\x1b[1m🚨 SeismoTail EEW Detection\x1b[0m");
    println!("\x1b[2m───────────────────────────────────────\x1b[0m");
    println!("  Algorithm: STA/LTA (Short-Term/Long-Term Average)");
    println!("  Threshold: {}", args.threshold);
    println!("\x1b[2m───────────────────────────────────────\x1b[0m\n");

    // Helper to print detections
    fn print_detections(detections: &[Detection]) {
        if detections.is_empty() {
            println!("  \x1b[92m✓ No significant seismic activity detected\x1b[0m");
        } else {
            println!("  \x1b[93mFound {} detection(s):\x1b[0m\n", detections.len());
            for det in detections {
                let alert_color = match det.alert_level {
                    AlertLevel::Severe => "\x1b[95m",
                    AlertLevel::Strong => "\x1b[91m",
                    AlertLevel::Moderate => "\x1b[93m",
                    AlertLevel::Light => "\x1b[92m",
                    _ => "\x1b[0m",
                };

                println!("  \x1b[1m{} EARTHQUAKE DETECTED!\x1b[0m", det.alert_level.emoji());
                println!("  ├─ Device:    {}", det.device_id);
                println!("  ├─ PGA:       {:.2} gals (cm/s²)", det.pga);
                println!("  ├─ STA/LTA:   {:.2}", det.sta_lta_ratio);
                println!("  ├─ Alert:     {}{}\x1b[0m", alert_color, det.alert_level.as_str().to_uppercase());
                if let Some(mag) = det.estimated_magnitude {
                    println!("  └─ Est. Mag:  ~M{:.1}", mag);
                }
                println!();
            }
        }
    }

    if args.simulate {
        // Simulate earthquake detection with synthetic waveform
        println!("\x1b[93m▶ Running detection on synthetic waveform...\x1b[0m\n");

        let detector = StaLtaDetector::default();

        // Simulate quiet background noise (~0.001g = ~1 gal)
        // OpenEEW data is in g (gravity units)
        let mut x: Vec<f32> = vec![0.001; 30];
        let mut y: Vec<f32> = vec![0.001; 30];
        let mut z: Vec<f32> = vec![0.001; 30];

        // Add P-wave arrival (sudden spike - 0.05g = ~50 gals for moderate quake)
        println!("  Simulating P-wave arrival...\n");
        x.extend(vec![0.02, 0.04, 0.08, 0.15, 0.25, 0.35, 0.30, 0.20, 0.10, 0.05]);
        y.extend(vec![0.01, 0.03, 0.06, 0.12, 0.20, 0.28, 0.24, 0.16, 0.08, 0.04]);
        z.extend(vec![0.01, 0.02, 0.04, 0.08, 0.12, 0.15, 0.12, 0.08, 0.05, 0.02]);
        // Decay back to quiet
        x.extend(vec![0.002; 10]);
        y.extend(vec![0.002; 10]);
        z.extend(vec![0.002; 10]);

        let record = AccelerometerRecord {
            device_id: "demo-sensor-001".to_string(),
            timestamp: chrono::Utc::now().timestamp() as f64,
            x, y, z,
            sr: 31.25,
        };

        let detections = detector.detect(&record);
        print_detections(&detections);

        println!("\x1b[2m───────────────────────────────────────\x1b[0m");
        println!("\x1b[1mPGA Reference Scale:\x1b[0m");
        println!("  ⚪ < 1 gal    │ Not felt");
        println!("  🟢 1-3 gals   │ Weak");
        println!("  🟡 3-10 gals  │ Light");
        println!("  🟠 10-50 gals │ Moderate (potential damage)");
        println!("  🔴 50-150 gals│ Strong (likely damage)");
        println!("  🟣 > 150 gals │ Severe (major damage)");
        println!("\x1b[2m───────────────────────────────────────\x1b[0m\n");

        println!("\x1b[92m✓ Simulation complete!\x1b[0m");
        println!("\n\x1b[2mTo analyze real OpenEEW earthquake data:\x1b[0m");
        println!("  seismotail detect --country mx --date 2018-02-16 --hour 23");

    } else if let Some(date) = &args.date {
        // Real data mode - fetch from OpenEEW S3
        println!("\x1b[93m▶ Fetching real data from OpenEEW (AWS S3)...\x1b[0m\n");
        println!("  Country: {}", args.country);
        println!("  Date:    {}", date);
        if let Some(hour) = &args.hour {
            println!("  Hour:    {}:00 UTC", hour);
        }
        println!("  Bucket:  s3://grillo-openeew/\n");

        let rt = tokio::runtime::Runtime::new()
            .context("failed to create tokio runtime")?;

        rt.block_on(async {
            let client = OpenEewClient::new().await;
            let detector = StaLtaDetector::default();

            println!("  \x1b[2mListing devices...\x1b[0m");
            
            match client.list_devices(&args.country).await {
                Ok(devices) => {
                    if devices.is_empty() {
                        println!("  \x1b[91m✗ No devices found for country: {}\x1b[0m", args.country);
                        return;
                    }

                    let device_limit = 5;
                    let files_per_device = 12; // Cover full hour (5-min intervals)
                    println!("  \x1b[92m✓ Found {} devices\x1b[0m", devices.len());
                    println!("  Analyzing {} devices, {} files each...\n", device_limit, files_per_device);

                    let mut all_detections = Vec::new();
                    let mut records_processed = 0;
                    let mut files_processed = 0;

                    for device_id in devices.iter().take(device_limit) {
                        print!("  Device {}... ", device_id);
                        
                        match client.list_files(&args.country, device_id, date, args.hour.as_deref()).await {
                            Ok(files) => {
                                if files.is_empty() {
                                    println!("\x1b[2mno data\x1b[0m");
                                    continue;
                                }
                                
                                let mut device_records = 0;
                                let mut max_pga: f32 = 0.0;
                                for key in files.iter().take(files_per_device) {
                                    match client.fetch_records(key).await {
                                        Ok(records) => {
                                            device_records += records.len();
                                            records_processed += records.len();
                                            files_processed += 1;
                                            
                                            for record in &records {
                                                // Track max PGA seen
                                                for i in 0..record.x.len().min(record.y.len()).min(record.z.len()) {
                                                    let pga = eew::StaLtaDetector::calculate_pga(
                                                        record.x[i], record.y[i], record.z[i]
                                                    );
                                                    if pga > max_pga { max_pga = pga; }
                                                }
                                                
                                                let dets = detector.detect(record);
                                                all_detections.extend(dets);
                                            }
                                        }
                                        Err(_) => continue,
                                    }
                                }
                                println!("\x1b[92m{} records, max PGA: {:.1} gals\x1b[0m", device_records, max_pga);
                            }
                            Err(_) => {
                                println!("\x1b[2mskipped\x1b[0m");
                            }
                        }
                    }

                    println!("\n\x1b[2m───────────────────────────────────────\x1b[0m");
                    println!("\x1b[1mResults:\x1b[0m");
                    println!("  Files processed:   {}", files_processed);
                    println!("  Records processed: {}", records_processed);
                    println!("  Detections found:  {}\n", all_detections.len());

                    print_detections(&all_detections);
                }
                Err(e) => {
                    println!("  \x1b[91m✗ Error: {}\x1b[0m", e);
                    println!("\n  \x1b[2mMake sure the date format is YYYY-MM-DD\x1b[0m");
                }
            }
        });
    } else {
        println!("\x1b[93m▶ Usage:\x1b[0m\n");
        println!("  # Run with synthetic waveform:");
        println!("  \x1b[96mseismotail detect --simulate\x1b[0m\n");
        println!("  # Analyze real OpenEEW earthquake data:");
        println!("  \x1b[96mseismotail detect --country mx --date 2018-02-16 --hour 23\x1b[0m\n");
        println!("\x1b[2mData: https://registry.opendata.aws/grillo-openeew/\x1b[0m");
    }

    Ok(())
}
