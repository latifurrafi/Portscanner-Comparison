## portscanner-comparison — Flags and Usage

This repo contains two scanners with mostly aligned flags:

- Go binary: `go/portscan-go`
- Rust binary: `rust/target/release/portscan-rs`

### Common flags

- `--host <string>`: Target host (IP or DNS). Required.
- `--start <int>`: Start port. Default: 1
- `--end <int>`: End port. Default: 1024
- `--workers <int>`: Max concurrent connection attempts. Default: 500
- `--timeout <ms>`: Max dial timeout per connection in milliseconds.
  - Default: 300
  - Upper bound when `--adaptive` is enabled (the derived timeout won’t exceed this).
- `--json` (bool): Output JSON list of open ports and banners. Default: false

### Adaptivity and reliability flags

- `--adaptive` (bool): Enable adaptive timeout per host.
  - Default: true
  - Behavior: Probes a small set of well-known ports to estimate RTT and sets `dialTimeout = clamp(3*medianRTT, 150ms..timeout)`.
  - Disable if you want a strict fixed timeout: `--adaptive=false`.

- `--retries <int>`: Number of retries when a dial times out.
  - Default: 1 (i.e., 2 total attempts including the first).
  - Each retry uses the same derived timeout.

### Examples

Fast local/LAN scan with adaptivity:
```bash
./go/portscan-go --host 192.168.1.10 --start 1 --end 1024 --workers 1000 --timeout 500 --adaptive --retries 1
```

WAN scan with stricter cap and fewer retries:
```bash
./rust/target/release/portscan-rs --host scanme.nmap.org --start 1 --end 1024 --workers 800 --timeout 1200 --adaptive --retries 0
```

JSON output (for tooling):
```bash
./go/portscan-go --host example.com --start 1 --end 1024 --workers 600 --timeout 800 --json
```

### Notes

- Both scanners perform full TCP connect scans and attempt a small banner read (200 ms) on open ports.
- Over the internet, prefer `--adaptive` with a reasonable `--timeout` to avoid long stalls.
- Increase `--workers` gradually; extremely high concurrency can hit OS limits or trigger rate limiting.



