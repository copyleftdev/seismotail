<div align="center">

# �� SeismoTail

**Real-time earthquake monitoring from your terminal**

https://github.com/user-attachments/assets/demo.mp4

</div>

## Install

```bash
cargo install seismotail
```

## Usage

```bash
# Fetch recent earthquakes
seismotail tail

# Stream live
seismotail live

# Web dashboard
seismotail ui
```

### Filtering

```bash
seismotail tail --min-magnitude 4.0
seismotail tail --radius 37.77,-122.42,500
seismotail tail --bbox 32,-125,42,-114
seismotail tail --format json | jq '.'
```

---

## Comparison

| Feature | SeismoTail | Earthquake-Live | GlobalQuake | OpenEEW |
|---------|------------|-----------------|-------------|---------|
| **Language** | Rust | Java | Java + CUDA | Node.js |
| **Type** | CLI + Web | CLI | Desktop GUI | IoT Toolkit |
| **Binary Size** | ~5 MB | JVM required | JVM required | N/A |
| **Startup Time** | <100ms | ~2s | ~5s | N/A |
| **Memory** | ~20 MB | ~200 MB | ~500 MB | N/A |
| **Real-time** | ✅ SSE streaming | ✅ Polling | ✅ Polling | ✅ Sensors |
| **Web UI** | ✅ Built-in | ❌ | ❌ | ✅ Dashboard |
| **Pipe-friendly** | ✅ JSON/NDJSON | ❌ | ❌ | ❌ |
| **Geo Filters** | ✅ bbox, radius | ✅ radius | ✅ | ❌ |
| **Early Warning** | ❌ | ❌ | ✅ EEW | ✅ EEW |
| **Hardware** | None | None | GPU optional | Sensors |
| **Data Source** | USGS | USGS | Multiple | Grillo sensors |
| **Status** | ✅ Active | 🔶 Dev | ❌ Discontinued | ✅ Active |

### Why SeismoTail?

- **Fast** — Single Rust binary, instant startup
- **Pipe-friendly** — JSON/NDJSON output for Unix pipelines  
- **Modern UI** — HTMX-powered web dashboard with dark mode
- **Lightweight** — No JVM, no runtime dependencies
- **Focused** — Does one thing well: consume USGS data

---

## License

MIT — Data from [USGS](https://earthquake.usgs.gov/) (public domain)
