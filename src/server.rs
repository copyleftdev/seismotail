//! Web server for the SeismoTail UI.
//!
//! Provides a real-time earthquake dashboard using:
//! - Axum for HTTP server
//! - SSE (Server-Sent Events) for real-time updates
//! - HTMX for dynamic UI without heavy JavaScript
//! - Material Design 3 inspired styling

use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse,
    },
    routing::{get, post},
    Router,
};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::client::{FeedType, UsgsClient};
use crate::filters::EventFilter;
use crate::models::Feature;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub feed_type: FeedType,
    pub poll_interval: u64,
    pub filter: EventFilter,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "127.0.0.1".to_string(),
            feed_type: FeedType::AllHour,
            poll_interval: 60,
            filter: EventFilter::default(),
        }
    }
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Channel for broadcasting events to SSE clients
    tx: broadcast::Sender<String>,
    /// Flag to control feed polling
    feed_active: Arc<AtomicBool>,
    /// Server configuration
    config: ServerConfig,
}

/// Create the Axum router with all routes.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/stream", get(sse_handler))
        .route("/events/recent", get(recent_events_handler))
        .route("/feed/start", post(start_feed_handler))
        .route("/feed/stop", post(stop_feed_handler))
        .route("/feed/status", get(feed_status_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}

/// Start the web server.
pub async fn run_server(config: ServerConfig) -> anyhow::Result<()> {
    // Create broadcast channel for SSE
    let (tx, _rx) = broadcast::channel::<String>(100);
    let feed_active = Arc::new(AtomicBool::new(true));

    let state = AppState {
        tx: tx.clone(),
        feed_active: feed_active.clone(),
        config: config.clone(),
    };

    // Spawn the background polling task
    let poll_state = state.clone();
    tokio::spawn(async move {
        poll_earthquakes(poll_state).await;
    });

    let app = create_router(state);

    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("🌍 SeismoTail UI starting at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Background task that polls USGS and broadcasts events.
async fn poll_earthquakes(state: AppState) {
    let client = match UsgsClient::new() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to create USGS client: {}", e);
            return;
        }
    };

    let mut seen_ids = std::collections::HashSet::new();

    loop {
        // Check if feed is active
        if !state.feed_active.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        match client.fetch_feed(state.config.feed_type) {
            Ok(feed) => {
                for event in feed.features {
                    // Deduplication
                    if seen_ids.contains(&event.id) {
                        continue;
                    }

                    // Apply filters
                    if !state.config.filter.matches(&event) {
                        continue;
                    }

                    seen_ids.insert(event.id.clone());

                    // Format as HTML for HTMX swap
                    let html = format_event_html(&event);
                    
                    // Broadcast to all SSE clients
                    let _ = state.tx.send(html);
                }
            }
            Err(e) => {
                tracing::warn!("Feed fetch failed: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(state.config.poll_interval)).await;
    }
}

/// Format an earthquake event as HTML.
fn format_event_html(event: &Feature) -> String {
    let mag = event.properties.mag.unwrap_or(0.0);
    let mag_type = event.properties.mag_type.as_deref().unwrap_or("?");
    let place = event.properties.place.as_deref().unwrap_or("Unknown location");
    let depth = event.depth_km();
    let severity_class = match mag {
        m if m >= 7.0 => "severity-critical",
        m if m >= 6.0 => "severity-major",
        m if m >= 4.5 => "severity-moderate",
        m if m >= 3.0 => "severity-light",
        _ => "severity-minor",
    };

    let severity_label = match mag {
        m if m >= 7.0 => "MAJOR",
        m if m >= 6.0 => "STRONG",
        m if m >= 4.5 => "MODERATE",
        m if m >= 3.0 => "LIGHT",
        m if m >= 2.0 => "MINOR",
        _ => "MICRO",
    };

    let lat = event.latitude();
    let lon = event.longitude();
    
    // Relative time (e.g., "2 hours ago")
    let relative_time = event.time()
        .map(|t| {
            let now = chrono::Utc::now();
            let diff = now.signed_duration_since(t);
            if diff.num_hours() < 1 {
                format!("{} min ago", diff.num_minutes().max(1))
            } else if diff.num_hours() < 24 {
                format!("{} hr ago", diff.num_hours())
            } else {
                format!("{} days ago", diff.num_days())
            }
        })
        .unwrap_or_else(|| "Unknown".to_string());

    // Build rich metadata pills
    let mut meta_pills = Vec::new();
    
    // Status (reviewed vs automatic)
    let status_class = if event.properties.status == "reviewed" { "reviewed" } else { "automatic" };
    let status_icon = if event.properties.status == "reviewed" { "✓" } else { "◐" };
    meta_pills.push(format!(
        r#"<span class="meta-pill {}">{} {}</span>"#,
        status_class, status_icon, event.properties.status
    ));
    
    // Felt reports
    if let Some(felt) = event.properties.felt {
        if felt > 0 {
            meta_pills.push(format!(
                r#"<span class="meta-pill felt">👥 {} felt</span>"#,
                felt
            ));
        }
    }
    
    // Community Intensity (CDI)
    if let Some(cdi) = event.properties.cdi {
        meta_pills.push(format!(
            r#"<span class="meta-pill intensity">📊 CDI {:.1}</span>"#,
            cdi
        ));
    }
    
    // Modified Mercalli Intensity (MMI)
    if let Some(mmi) = event.properties.mmi {
        meta_pills.push(format!(
            r#"<span class="meta-pill intensity">📈 MMI {:.1}</span>"#,
            mmi
        ));
    }
    
    // Significance (high = 500+)
    let sig = event.properties.sig;
    if sig >= 500 {
        meta_pills.push(format!(
            r#"<span class="meta-pill sig-high">⚡ sig {}</span>"#,
            sig
        ));
    } else if sig >= 100 {
        meta_pills.push(format!(
            r#"<span class="meta-pill">⚡ sig {}</span>"#,
            sig
        ));
    }
    
    // Number of stations
    if let Some(nst) = event.properties.nst {
        meta_pills.push(format!(
            r#"<span class="meta-pill">📡 {} stations</span>"#,
            nst
        ));
    }
    
    // Azimuthal gap
    if let Some(gap) = event.properties.gap {
        meta_pills.push(format!(
            r#"<span class="meta-pill">◔ gap {:.0}°</span>"#,
            gap
        ));
    }
    
    // Network
    meta_pills.push(format!(
        r#"<span class="meta-pill">🌐 {}</span>"#,
        event.properties.net
    ));
    
    let meta_html = meta_pills.join("\n        ");

    format!(
        r#"<div class="event-card {severity_class}" id="event-{id}">
  <div class="event-row">
    <div class="event-mag">
      <span class="mag-value">{mag:.1}</span>
      <span class="mag-type">{mag_type}</span>
    </div>
    
    <div class="event-main">
      <div class="event-title-row">
        <span class="event-place">{place}</span>
        <span class="badge badge-severity">{severity_label}</span>
        {tsunami_badge}
        {alert_badge}
      </div>
      
      <div class="event-basic-meta">
        <span class="basic-meta-item">
          <span class="icon">↓</span> {depth:.0} km
        </span>
        <span class="basic-meta-item">
          <span class="icon">◷</span> {relative_time}
        </span>
        <span class="basic-meta-item">
          <span class="icon">⊕</span> {lat:.2}°, {lon:.2}°
        </span>
      </div>
      
      <div class="event-meta">
        {meta_html}
      </div>
    </div>
    
    <div class="event-map-container" id="map-{id}"></div>
  </div>
</div>
<script>
(function() {{
  var el = document.getElementById('map-{id}');
  if (!el || el._leaflet_id) return;
  var map = L.map('map-{id}', {{
    zoomControl: false,
    attributionControl: false,
    dragging: false,
    scrollWheelZoom: false,
    doubleClickZoom: false
  }}).setView([{lat}, {lon}], 4);
  L.tileLayer('https://{{s}}.basemaps.cartocdn.com/dark_all/{{z}}/{{x}}/{{y}}{{r}}.png').addTo(map);
  L.circleMarker([{lat}, {lon}], {{
    radius: 6,
    fillColor: '{marker_color}',
    color: 'rgba(255,255,255,0.8)',
    weight: 2,
    opacity: 1,
    fillOpacity: 0.9
  }}).addTo(map);
}})();
</script>"#,
        id = event.id,
        mag = mag,
        mag_type = mag_type,
        severity_label = severity_label,
        severity_class = severity_class,
        tsunami_badge = if event.properties.tsunami != 0 {
            r#"<span class="badge badge-tsunami">🌊 Tsunami</span>"#
        } else { "" },
        alert_badge = match event.properties.alert.as_deref() {
            Some("red") => r#"<span class="badge badge-alert badge-alert-red">⚠ Red Alert</span>"#,
            Some("orange") => r#"<span class="badge badge-alert badge-alert-orange">⚠ Orange</span>"#,
            Some("yellow") => r#"<span class="badge badge-alert badge-alert-yellow">⚠ Yellow</span>"#,
            Some("green") => r#"<span class="badge badge-alert badge-alert-green">✓ Green</span>"#,
            _ => "",
        },
        place = place,
        depth = depth,
        relative_time = relative_time,
        lat = lat,
        lon = lon,
        meta_html = meta_html,
        marker_color = match mag {
            m if m >= 7.0 => "#ef4444",
            m if m >= 6.0 => "#f97316",
            m if m >= 4.5 => "#06b6d4",
            m if m >= 3.0 => "#10b981",
            _ => "#6b7280",
        },
    )
}

// ============================================================================
// Route Handlers
// ============================================================================

/// Main page handler - serves the HTML UI.
async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// SSE stream handler for real-time events.
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(html) => Some(Ok(Event::default().event("earthquake").data(html))),
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Start the feed handler.
async fn start_feed_handler(State(state): State<AppState>) -> impl IntoResponse {
    state.feed_active.store(true, Ordering::Relaxed);
    tracing::info!("Feed started via UI");
    Html(r#"<div id="feed-status" class="status-pill"><span class="status-dot"></span><span>Live</span></div>"#)
}

/// Stop the feed handler.
async fn stop_feed_handler(State(state): State<AppState>) -> impl IntoResponse {
    state.feed_active.store(false, Ordering::Relaxed);
    tracing::info!("Feed stopped via UI");
    Html(r#"<div id="feed-status" class="status-pill status-paused"><span class="status-dot"></span><span>Paused</span></div>"#)
}

/// Feed status handler.
async fn feed_status_handler(State(state): State<AppState>) -> Html<String> {
    let is_active = state.feed_active.load(Ordering::Relaxed);
    if is_active {
        Html(r#"<div id="feed-status" class="status-pill"><span class="status-dot"></span><span>Live</span></div>"#.to_string())
    } else {
        Html(r#"<div id="feed-status" class="status-pill status-paused"><span class="status-dot"></span><span>Paused</span></div>"#.to_string())
    }
}

/// Health check endpoint.
async fn health_handler() -> &'static str {
    "OK"
}

/// Recent events handler - fetches current events for initial page load.
async fn recent_events_handler(State(state): State<AppState>) -> Html<String> {
    let client = match UsgsClient::new() {
        Ok(c) => c,
        Err(_) => return Html("<div class='error'>Failed to fetch events</div>".to_string()),
    };

    match client.fetch_feed(state.config.feed_type) {
        Ok(feed) => {
            let mut html = String::new();
            let mut count = 0;
            
            for event in feed.features.iter().take(20) {
                // Apply filters
                if !state.config.filter.matches(event) {
                    continue;
                }
                
                html.push_str(&format_event_html(event));
                count += 1;
            }
            
            if count == 0 {
                html = "<div class='empty-state'><div class='icon'>🌍</div><p>No earthquakes match your filters</p></div>".to_string();
            }
            
            Html(html)
        }
        Err(_) => Html("<div class='error'>Failed to fetch events</div>".to_string()),
    }
}

// ============================================================================
// HTML Template (embedded for single-binary deployment)
// ============================================================================

const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en" data-theme="dark">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>SeismoTail — Real-time Earthquake Monitor</title>
    
    <!-- Modern Font -->
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
    
    <!-- HTMX + SSE -->
    <script src="https://unpkg.com/htmx.org@1.9.10"></script>
    <script src="https://unpkg.com/htmx.org@1.9.10/dist/ext/sse.js"></script>
    
    <!-- Leaflet -->
    <link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" />
    <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
    
    <style>
        /* =============================================
           2025 Premium UI — Inspired by Linear/Vercel
           ============================================= */
        
        :root {
            --font: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
            
            /* Light Theme */
            --bg-primary: #ffffff;
            --bg-secondary: #f8fafc;
            --bg-tertiary: #f1f5f9;
            --bg-elevated: #ffffff;
            --bg-hover: #f1f5f9;
            
            --text-primary: #0f172a;
            --text-secondary: #475569;
            --text-tertiary: #94a3b8;
            
            --border: #e2e8f0;
            --border-hover: #cbd5e1;
            
            --accent: #6366f1;
            --accent-hover: #4f46e5;
            --accent-soft: rgba(99, 102, 241, 0.1);
            
            --success: #10b981;
            --warning: #f59e0b;
            --danger: #ef4444;
            
            --shadow-sm: 0 1px 2px rgba(0,0,0,0.05);
            --shadow-md: 0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -2px rgba(0,0,0,0.1);
            --shadow-lg: 0 10px 15px -3px rgba(0,0,0,0.1), 0 4px 6px -4px rgba(0,0,0,0.1);
            --shadow-glow: 0 0 20px rgba(99, 102, 241, 0.15);
            
            --radius-sm: 6px;
            --radius-md: 10px;
            --radius-lg: 16px;
            --radius-full: 9999px;
        }
        
        [data-theme="dark"] {
            --bg-primary: #09090b;
            --bg-secondary: #0f0f12;
            --bg-tertiary: #18181b;
            --bg-elevated: #1c1c1f;
            --bg-hover: #27272a;
            
            --text-primary: #fafafa;
            --text-secondary: #a1a1aa;
            --text-tertiary: #52525b;
            
            --border: #27272a;
            --border-hover: #3f3f46;
            
            --accent: #818cf8;
            --accent-hover: #6366f1;
            --accent-soft: rgba(129, 140, 248, 0.1);
            
            --shadow-sm: 0 1px 2px rgba(0,0,0,0.3);
            --shadow-md: 0 4px 6px -1px rgba(0,0,0,0.4);
            --shadow-lg: 0 10px 15px -3px rgba(0,0,0,0.5);
            --shadow-glow: 0 0 30px rgba(129, 140, 248, 0.1);
        }
        
        * { margin: 0; padding: 0; box-sizing: border-box; }
        
        html { scroll-behavior: smooth; }
        
        body {
            font-family: var(--font);
            background: var(--bg-primary);
            color: var(--text-primary);
            line-height: 1.6;
            min-height: 100vh;
            -webkit-font-smoothing: antialiased;
            -moz-osx-font-smoothing: grayscale;
        }
        
        /* Subtle animated gradient background */
        body::before {
            content: '';
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            height: 400px;
            background: radial-gradient(ellipse 80% 50% at 50% -20%, var(--accent-soft), transparent);
            pointer-events: none;
            z-index: -1;
        }
        
        /* ===== HEADER ===== */
        .header {
            position: sticky;
            top: 0;
            z-index: 1000;
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
            background: rgba(9, 9, 11, 0.8);
            border-bottom: 1px solid var(--border);
        }
        
        [data-theme="light"] .header {
            background: rgba(255, 255, 255, 0.8);
        }
        
        .header-inner {
            max-width: 1400px;
            margin: 0 auto;
            padding: 0.875rem 1.5rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        
        .logo {
            display: flex;
            align-items: center;
            gap: 0.75rem;
            font-weight: 600;
            font-size: 1.125rem;
            color: var(--text-primary);
            text-decoration: none;
            letter-spacing: -0.02em;
        }
        
        .logo:hover .logo-icon {
            transform: scale(1.05);
        }
        
        .logo-icon {
            width: 32px;
            height: 32px;
            transition: transform 0.2s ease;
        }
        
        .logo-icon svg {
            width: 100%;
            height: 100%;
        }
        
        .header-actions {
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }
        
        .status-pill {
            display: flex;
            align-items: center;
            gap: 0.5rem;
            padding: 0.375rem 0.875rem;
            border-radius: var(--radius-full);
            font-size: 0.8125rem;
            font-weight: 500;
            background: var(--bg-tertiary);
            border: 1px solid var(--border);
        }
        
        .status-dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
            background: var(--success);
            animation: pulse 2s infinite;
        }
        
        @keyframes pulse {
            0%, 100% { opacity: 1; transform: scale(1); }
            50% { opacity: 0.5; transform: scale(0.9); }
        }
        
        .status-paused .status-dot {
            background: var(--warning);
            animation: none;
        }
        
        .btn {
            display: inline-flex;
            align-items: center;
            gap: 0.375rem;
            padding: 0.5rem 1rem;
            border-radius: var(--radius-md);
            font-size: 0.8125rem;
            font-weight: 500;
            border: none;
            cursor: pointer;
            transition: all 0.15s ease;
            font-family: var(--font);
        }
        
        .btn-ghost {
            background: transparent;
            color: var(--text-secondary);
            border: 1px solid var(--border);
        }
        
        .btn-ghost:hover {
            background: var(--bg-hover);
            border-color: var(--border-hover);
            color: var(--text-primary);
        }
        
        .btn-primary {
            background: var(--accent);
            color: white;
        }
        
        .btn-primary:hover {
            background: var(--accent-hover);
            transform: translateY(-1px);
            box-shadow: var(--shadow-md);
        }
        
        .theme-toggle {
            width: 36px;
            height: 36px;
            border-radius: var(--radius-md);
            border: 1px solid var(--border);
            background: var(--bg-tertiary);
            cursor: pointer;
            display: flex;
            align-items: center;
            justify-content: center;
            transition: all 0.15s;
        }
        
        .theme-toggle:hover {
            background: var(--bg-hover);
            border-color: var(--border-hover);
        }
        
        /* ===== MAIN ===== */
        .main {
            max-width: 1400px;
            margin: 0 auto;
            padding: 2rem 1.5rem;
        }
        
        .section-header {
            display: flex;
            justify-content: space-between;
            align-items: flex-end;
            margin-bottom: 1.5rem;
        }
        
        .section-title {
            font-size: 1.5rem;
            font-weight: 600;
            letter-spacing: -0.025em;
        }
        
        .section-subtitle {
            font-size: 0.875rem;
            color: var(--text-tertiary);
            margin-top: 0.25rem;
        }
        
        /* ===== EVENT FEED ===== */
        .event-feed {
            display: grid;
            gap: 1rem;
        }
        
        .event-card {
            position: relative;
            background: var(--bg-elevated);
            border: 1px solid var(--border);
            border-radius: var(--radius-lg);
            padding: 1.25rem;
            transition: all 0.2s ease;
            animation: cardSlide 0.4s ease-out;
        }
        
        @keyframes cardSlide {
            from { opacity: 0; transform: translateY(-8px); }
            to { opacity: 1; transform: translateY(0); }
        }
        
        .event-card:hover {
            border-color: var(--border-hover);
            box-shadow: var(--shadow-md);
            transform: translateY(-2px);
        }
        
        .event-card.severity-critical {
            border-left: 3px solid #ef4444;
            background: linear-gradient(90deg, rgba(239,68,68,0.05) 0%, var(--bg-elevated) 30%);
        }
        
        .event-card.severity-major {
            border-left: 3px solid #f97316;
            background: linear-gradient(90deg, rgba(249,115,22,0.05) 0%, var(--bg-elevated) 30%);
        }
        
        .event-card.severity-moderate {
            border-left: 3px solid #06b6d4;
        }
        
        .event-card.severity-light {
            border-left: 3px solid #10b981;
        }
        
        .event-card.severity-minor {
            border-left: 3px solid var(--border);
        }
        
        .event-row {
            display: flex;
            gap: 1.25rem;
            align-items: flex-start;
        }
        
        .event-mag {
            flex-shrink: 0;
            width: 64px;
            height: 64px;
            border-radius: var(--radius-md);
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            background: var(--bg-tertiary);
            border: 1px solid var(--border);
        }
        
        .mag-value {
            font-size: 1.5rem;
            font-weight: 700;
            line-height: 1;
            letter-spacing: -0.05em;
        }
        
        .mag-type {
            font-size: 0.625rem;
            font-weight: 500;
            color: var(--text-tertiary);
            text-transform: uppercase;
            margin-top: 0.125rem;
        }
        
        .severity-critical .mag-value { color: #ef4444; }
        .severity-major .mag-value { color: #f97316; }
        .severity-moderate .mag-value { color: #06b6d4; }
        .severity-light .mag-value { color: #10b981; }
        
        .event-main {
            flex: 1;
            min-width: 0;
        }
        
        .event-title-row {
            display: flex;
            align-items: center;
            gap: 0.5rem;
            flex-wrap: wrap;
            margin-bottom: 0.5rem;
        }
        
        .event-place {
            font-weight: 500;
            font-size: 0.9375rem;
            color: var(--text-primary);
        }
        
        .badge {
            display: inline-flex;
            align-items: center;
            gap: 0.25rem;
            padding: 0.125rem 0.5rem;
            border-radius: var(--radius-sm);
            font-size: 0.6875rem;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.025em;
        }
        
        .badge-severity {
            background: var(--bg-tertiary);
            color: var(--text-secondary);
        }
        
        .badge-tsunami {
            background: rgba(6, 182, 212, 0.15);
            color: #06b6d4;
        }
        
        .badge-alert {
            color: white;
        }
        
        .badge-alert-red { background: #ef4444; }
        .badge-alert-orange { background: #f97316; }
        .badge-alert-yellow { background: #eab308; color: #1c1917; }
        .badge-alert-green { background: #10b981; }
        
        .event-meta {
            display: flex;
            flex-wrap: wrap;
            gap: 0.5rem;
            margin-top: 0.75rem;
        }
        
        .meta-pill {
            display: inline-flex;
            align-items: center;
            gap: 0.25rem;
            padding: 0.25rem 0.5rem;
            border-radius: var(--radius-sm);
            font-size: 0.6875rem;
            font-weight: 500;
            background: var(--bg-tertiary);
            color: var(--text-secondary);
            border: 1px solid var(--border);
        }
        
        .meta-pill .icon {
            opacity: 0.7;
        }
        
        .meta-pill.reviewed {
            background: rgba(16, 185, 129, 0.1);
            border-color: rgba(16, 185, 129, 0.3);
            color: #10b981;
        }
        
        .meta-pill.automatic {
            background: rgba(245, 158, 11, 0.1);
            border-color: rgba(245, 158, 11, 0.3);
            color: #f59e0b;
        }
        
        .meta-pill.felt {
            background: rgba(99, 102, 241, 0.1);
            border-color: rgba(99, 102, 241, 0.3);
            color: var(--accent);
        }
        
        .meta-pill.intensity {
            background: rgba(239, 68, 68, 0.1);
            border-color: rgba(239, 68, 68, 0.3);
            color: #ef4444;
        }
        
        .meta-pill.sig-high {
            background: rgba(239, 68, 68, 0.1);
            border-color: rgba(239, 68, 68, 0.3);
            color: #ef4444;
        }
        
        .event-basic-meta {
            display: flex;
            flex-wrap: wrap;
            gap: 1rem;
            font-size: 0.8125rem;
            color: var(--text-tertiary);
            margin-bottom: 0.5rem;
        }
        
        .basic-meta-item {
            display: flex;
            align-items: center;
            gap: 0.375rem;
        }
        
        .basic-meta-item .icon {
            opacity: 0.6;
        }
        
        .event-map-container {
            flex-shrink: 0;
            width: 140px;
            height: 100px;
            border-radius: var(--radius-md);
            overflow: hidden;
            border: 1px solid var(--border);
        }
        
        .event-map-container .leaflet-control-attribution { display: none; }
        
        /* ===== EMPTY STATE ===== */
        .empty-state {
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            padding: 4rem 2rem;
            text-align: center;
        }
        
        .empty-icon {
            width: 64px;
            height: 64px;
            border-radius: 50%;
            background: var(--bg-tertiary);
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.5rem;
            margin-bottom: 1rem;
            animation: spin 2s linear infinite;
        }
        
        @keyframes spin {
            from { transform: rotate(0deg); }
            to { transform: rotate(360deg); }
        }
        
        .empty-title {
            font-weight: 500;
            color: var(--text-primary);
            margin-bottom: 0.25rem;
        }
        
        .empty-desc {
            font-size: 0.875rem;
            color: var(--text-tertiary);
        }
        
        /* ===== FOOTER ===== */
        .footer {
            border-top: 1px solid var(--border);
            padding: 1.5rem;
            text-align: center;
            font-size: 0.8125rem;
            color: var(--text-tertiary);
        }
        
        .footer a {
            color: var(--text-secondary);
            text-decoration: none;
            transition: color 0.15s;
        }
        
        .footer a:hover {
            color: var(--accent);
        }
        
        /* ===== RESPONSIVE ===== */
        @media (max-width: 768px) {
            .header-inner { padding: 0.75rem 1rem; }
            .main { padding: 1.25rem 1rem; }
            .event-row { flex-direction: column; }
            .event-map-container { width: 100%; height: 140px; }
            .event-mag { width: 56px; height: 56px; }
            .mag-value { font-size: 1.25rem; }
        }
    </style>
</head>
<body>
    <header class="header">
        <div class="header-inner">
            <a href="/" class="logo">
                <div class="logo-icon">
                    <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <defs>
                            <linearGradient id="logoGradient" x1="0%" y1="0%" x2="100%" y2="100%">
                                <stop offset="0%" style="stop-color:#818cf8"/>
                                <stop offset="100%" style="stop-color:#c084fc"/>
                            </linearGradient>
                        </defs>
                        <!-- Outer ring -->
                        <circle cx="16" cy="16" r="14" stroke="url(#logoGradient)" stroke-width="2" fill="none" opacity="0.3"/>
                        <!-- Middle ring -->
                        <circle cx="16" cy="16" r="9" stroke="url(#logoGradient)" stroke-width="2" fill="none" opacity="0.6"/>
                        <!-- Inner pulse -->
                        <circle cx="16" cy="16" r="4" fill="url(#logoGradient)"/>
                        <!-- Seismic wave -->
                        <path d="M4 16 L8 16 L10 12 L12 20 L14 14 L16 18 L18 15 L20 17 L22 13 L24 19 L26 16 L28 16" 
                              stroke="url(#logoGradient)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
                    </svg>
                </div>
                <span>SeismoTail</span>
            </a>
            
            <div class="header-actions">
                <div id="feed-status" class="status-pill" hx-get="/feed/status" hx-trigger="load">
                    <span class="status-dot"></span>
                    <span>Connecting</span>
                </div>
                
                <button class="btn btn-ghost" hx-post="/feed/stop" hx-target="#feed-status" hx-swap="outerHTML">
                    ⏸ Pause
                </button>
                
                <button class="btn btn-primary" hx-post="/feed/start" hx-target="#feed-status" hx-swap="outerHTML">
                    ▶ Resume
                </button>
                
                <button class="theme-toggle" onclick="toggleTheme()" title="Toggle theme">
                    🌙
                </button>
            </div>
        </div>
    </header>
    
    <main class="main">
        <div class="section-header">
            <div>
                <h1 class="section-title">Live Earthquake Feed</h1>
                <p class="section-subtitle">Real-time seismic activity from USGS</p>
            </div>
        </div>
        
        <div class="event-feed" 
             id="event-feed"
             hx-ext="sse" 
             sse-connect="/stream" 
             sse-swap="earthquake"
             hx-swap="afterbegin"
             hx-get="/events/recent"
             hx-trigger="load"
             hx-swap="innerHTML">
            
            <div class="empty-state">
                <div class="empty-icon">◐</div>
                <p class="empty-title">Loading seismic data</p>
                <p class="empty-desc">Fetching recent earthquakes...</p>
            </div>
        </div>
    </main>
    
    <footer class="footer">
        <p>Data from <a href="https://earthquake.usgs.gov/" target="_blank">USGS Earthquake Hazards Program</a> · SeismoTail v0.1.0</p>
    </footer>
    
    <script>
        function toggleTheme() {
            const html = document.documentElement;
            const current = html.getAttribute('data-theme');
            const next = current === 'dark' ? 'light' : 'dark';
            html.setAttribute('data-theme', next);
            document.querySelector('.theme-toggle').textContent = next === 'dark' ? '🌙' : '☀️';
            localStorage.setItem('theme', next);
        }
        
        // Load saved theme
        const savedTheme = localStorage.getItem('theme') || 'dark';
        document.documentElement.setAttribute('data-theme', savedTheme);
        document.querySelector('.theme-toggle').textContent = savedTheme === 'dark' ? '🌙' : '☀️';
        
        // Remove loading state on first event
        document.body.addEventListener('htmx:afterSwap', function(e) {
            if (e.detail.target.id === 'event-feed') {
                document.querySelectorAll('.empty-state').forEach(el => el.remove());
            }
        });
    </script>
</body>
</html>
"##;
