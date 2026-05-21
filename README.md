# osu!tourney Data Reader

Reads data from [osu!tourney](https://osu.ppy.sh/wiki/en/osu%21_tournament_client/osu%21tourney) and exposes per-player scores via WebSocket/HTTP.

Created for the [DACH Open: Interim Masters](https://osu.ppy.sh/community/forums/topics/2153164?n=1) tournament and intended to be paired with [osu-dach](https://github.com/medylme/osu-dach), an osu!(lazer) fork that uses the per-player granularity to handle custom EZ/EZHD score multipliers in the overlay.

## Requirements

- Windows (x86_64)
- [osu!tourney](https://osu.ppy.sh/wiki/en/osu%21_tournament_client/osu%21tourney)

## Usage

1. Grab the latest release from the [Releases](https://github.com/medylme/osu-tourney-data-reader/releases/latest) page, or build from source.
2. Run osu!tourney; wait for every instance to initialize
3. Run osu-tourney-data-reader

### Command-Line Arguments

| Argument          | Description                  |
| ----------------- | ---------------------------- |
| `-p`, `--port`    | Server port (default: 25050) |
| `-v`, `--verbose` | Enable debug logging         |

## Building

```bash
cargo build --release
```

or

```bash
just dist
```

## Special Thanks

[gosumemory](https://github.com/l3lackShark/gosumemory) for memory traversal strategy and offsets.

## License

GPLv3
