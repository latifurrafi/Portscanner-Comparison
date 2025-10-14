## Portscanner Comparison (Go vs Rust)

A minimal, high-concurrency TCP port scanner implemented in two languages to compare ergonomics and performance:

- Go (`go/main.go`): goroutine-based concurrent dialer
- Rust (`rust/src/main.rs`): Tokio-based async dialer

Both attempt a short banner read on open ports. A `benchmark.sh` script can run multiple worker counts and record timing/memory stats to `benchmark_results.csv`.

### How it works

- You pass a `--host` and a port range (`--start`, `--end`).
- The scanner dials each port concurrently (bounded by `--workers`).
- Successful connections are reported as open; the scanner tries to read up to 256 bytes for a banner with a ~200ms read deadline.
- Output can be pretty-printed JSON with `--json`, or human-readable text.

### Requirements

- Go 1.25+ (for the Go scanner)
- Rust toolchain (for the Rust scanner)
- GNU time (`/usr/bin/time` or `gtime`) for benchmarking
- Optional: `jq` to count JSON results in the benchmark

### Clone

```bash
git clone https://github.com/your-username/portscanner-comparison.git
cd portscanner-comparison
```

### Build

- Go binary:

```bash
cd go
go build -ldflags "-s -w" -o portscan-go main.go
cd ..
```

- Rust binary (release):

```bash
cd rust
cargo build --release
cd ..
```

This will produce:

- `go/portscan-go`
- `rust/target/release/portscan-rs`

### Usage

- Go scanner:

```bash
./go/portscan-go --host scanme.nmap.org --start 1 --end 1024 --workers 500 --timeout 300
```

- Rust scanner:

```bash
./rust/target/release/portscan-rs --host scanme.nmap.org --start 1 --end 1024 --workers 500 --timeout 300
```

Add `--json` to either command for JSON output.

### Benchmark

After building at least one binary, run:

```bash
./benchmark.sh scanme.nmap.org 1 1024
```

This will:

- Detect available binaries among `go/portscan-go`, `go/portscan-epoll`, and `rust/target/release/portscan-rs`.
- Sweep worker counts and repeat runs.
- Write results to `benchmark_results.csv` with timing, memory, and open-port counts.

Open the CSV in your favorite tool to compare `wall_seconds` and `max_rss_kb`.

### Notes

- Increase `--end` and `--workers` cautiously; very high concurrency can run into OS limits.
- `go/portscan-epoll` is optional and Linux-only (build it similarly if you add an epoll version).


