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

# EEW detection demo
seismotail detect --demo
```

### Filtering

```bash
seismotail tail --min-magnitude 4.0
seismotail tail --radius 37.77,-122.42,500
seismotail tail --format json | jq '.'
```

### EEW Detection (Experimental)

```bash
# Run STA/LTA earthquake detection on simulated data
seismotail detect --demo

# Output:
# 🟠 EARTHQUAKE DETECTED!
# ├─ PGA:       19.85 gals (cm/s²)
# ├─ STA/LTA:   5.79
# ├─ Alert:     MODERATE
# └─ Est. Mag:  ~M3.8
```

Uses the industry-standard **STA/LTA algorithm** (Short-Term Average / Long-Term Average) 
for P-wave detection — the same technique used by OpenEEW and professional seismic networks.

---

## Comparison

| Feature | SeismoTail | Earthquake-Live | GlobalQuake | OpenEEW |
|---------|------------|-----------------|-------------|---------|
| **Language** | Rust | Java | Java + CUDA | Node.js |
| **Type** | CLI + Web | CLI | Desktop GUI | IoT Toolkit |
| **Binary Size** | ~5 MB | JVM required | JVM required | N/A |
| **Real-time** | ✅ SSE streaming | ✅ Polling | ✅ Polling | ✅ Sensors |
| **EEW Detection** | ✅ STA/LTA | ❌ | ✅ | ✅ |
| **Web UI** | ✅ Built-in | ❌ | ❌ | ✅ Dashboard |
| **Pipe-friendly** | ✅ JSON/NDJSON | ❌ | ❌ | ❌ |
| **Hardware** | None | None | GPU optional | Sensors |
| **Data Source** | USGS + OpenEEW | USGS | Multiple | Grillo sensors |

---

## License

MIT — Data from [USGS](https://earthquake.usgs.gov/) and [OpenEEW](https://openeew.com/) (public domain)
