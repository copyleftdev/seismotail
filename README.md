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

## License

MIT — Data from [USGS Earthquake Hazards Program](https://earthquake.usgs.gov/) (public domain)
