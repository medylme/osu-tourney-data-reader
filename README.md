# osu!tourney Data Reader

Reads data from [osu!tourney](https://osu.ppy.sh/wiki/en/osu%21_tournament_client/osu%21tourney) and exposes it via WebSocket/HTTP.

Created during [DACH Open: Interim Masters](https://osu.ppy.sh/community/forums/topics/2153164?n=1) to handle custom EZ(HD) multipliers in [osu-dach](https://github.com/medylme/osu-dach), an osu!(lazer) tournament client fork.

## Requirements

- Windows (x86_64)
- osu!tourney

## Usage

1. Start osu!tourney and wait for every instance to initialize
2. Run the executable; starts on port `25050` by default

### Command-Line Arguments

| Argument          | Description                  |
| ----------------- | ---------------------------- |
| `-p`, `--port`    | Server port (default: 25050) |
| `-v`, `--verbose` | Enable debug logging         |

## Building from Source

```bash
cargo build --release
```

or

```bash
just dist
```

## Special Thanks

[gosumemory](https://github.com/l3lackShark/gosumemory) for memory reading strategy and offsets.

## License

GPLv3
