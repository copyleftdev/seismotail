<div align="center">

# 🌍 SeismoTail

**Real-time earthquake monitoring & early warning from your terminal**

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

## Install

```bash
cargo install seismotail
```

## Usage

```bash
# Fetch recent earthquakes
seismotail tail

# Stream live updates
seismotail live

# Web dashboard with map
seismotail ui

# EEW detection (see below)
seismotail detect --simulate
```

### Filtering

```bash
seismotail tail --min-magnitude 4.0
seismotail tail --max-depth 70
seismotail tail --radius 37.77,-122.42,500   # lat,lon,km
seismotail tail --format json | jq '.'
```

---

## 🚨 Earthquake Early Warning (EEW)

SeismoTail includes a full **STA/LTA P-wave detector** that can analyze real accelerometer data from the [OpenEEW](https://openeew.com/) public dataset on AWS.

### Analyze Real Earthquakes

```bash
# Analyze the 2018-02-16 M7.2 Oaxaca, Mexico earthquake
seismotail detect --country mx --date 2018-02-16 --hour 23

# Output:
# 🟣 EARTHQUAKE DETECTED!
# ├─ Device:    000
# ├─ PGA:       4396.69 gals (cm/s²)
# ├─ STA/LTA:   2.19
# ├─ Alert:     SEVERE
# └─ Est. Mag:  ~M6.1
```

### Run with Synthetic Data

```bash
seismotail detect --simulate
```

### How It Works

| Component | Description |
|-----------|-------------|
| **Algorithm** | STA/LTA (Short-Term/Long-Term Average ratio) |
| **Data Source** | [OpenEEW AWS S3](https://registry.opendata.aws/grillo-openeew/) (free, public) |
| **Countries** | 🇲🇽 Mexico, 🇨🇱 Chile, 🇨🇷 Costa Rica, 🇳🇿 New Zealand, 🇵🇷 Puerto Rico |
| **Hardware** | None required — uses historical accelerometer data |

### Alert Levels (PGA-based)

| Emoji | Level | PGA (gals) | Effect |
|-------|-------|------------|--------|
| ⚪ | None | < 1 | Not felt |
| 🟢 | Weak | 1-3 | May be felt |
| 🟡 | Light | 3-10 | Indoor objects shake |
| 🟠 | Moderate | 10-50 | Potential damage |
| 🔴 | Strong | 50-150 | Likely damage |
| 🟣 | Severe | > 150 | Major damage |

---

## Comparison

| Feature | SeismoTail | Earthquake-Live | GlobalQuake | OpenEEW |
|---------|------------|-----------------|-------------|---------|
| **Language** | Rust | Java | Java + CUDA | Node.js |
| **Type** | CLI + Web + EEW | CLI | Desktop GUI | IoT Toolkit |
| **Binary Size** | ~8 MB | JVM required | JVM required | N/A |
| **Real-time USGS** | ✅ SSE streaming | ✅ Polling | ✅ Polling | ❌ |
| **EEW Detection** | ✅ STA/LTA | ❌ | ✅ | ✅ |
| **Analyze OpenEEW Data** | ✅ S3 fetch | ❌ | ❌ | ✅ (own sensors) |
| **Web UI** | ✅ Built-in | ❌ | ❌ | ✅ Dashboard |
| **Pipe-friendly** | ✅ JSON/NDJSON | ❌ | ❌ | ❌ |
| **Hardware Required** | None | None | GPU optional | Sensors |

---

## Data Sources

- **USGS GeoJSON Feeds** — Real-time earthquake catalog (public domain)
- **OpenEEW on AWS** — Accelerometer recordings from Grillo sensors (public)

## License

MIT — See [LICENSE](LICENSE)
