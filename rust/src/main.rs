// src/main.rs
use clap::Parser;
use serde::Serialize;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::timeout;

fn estimate_timeout(ip: &str, max_timeout: Duration) -> Duration {
    let samples = [22u16, 80u16, 443u16, 53u16, 25u16];
    let mut durations = Vec::with_capacity(samples.len());
    for sp in samples { 
        let addr = format!("{}:{}", ip, sp);
        let start = std::time::Instant::now();
        // Use blocking connect in a tiny tokio::task::block_in_place? Simpler: best-effort async with small timeout
        // This function runs before we spawn many tasks, so a small block is fine.
        let _ = std::net::TcpStream::connect_timeout(&addr.parse().ok().unwrap_or_else(|| std::net::SocketAddr::from(([127,0,0,1], sp))), std::time::Duration::from_millis(200));
        let d = start.elapsed();
        durations.push(d);
    }
    durations.sort();
    let median = durations[durations.len()/2];
    let floor = Duration::from_millis(150);
    let mut derived = median * 3;
    if derived < floor { derived = floor; }
    if derived > max_timeout { derived = max_timeout; }
    derived
}

#[derive(Parser, Debug)]
#[command(author, version, about = "High-performance concurrent port scanner (Rust + Tokio)")]
struct Args {
    /// target host (ip or domain)
    #[arg(long)]
    host: String,

    #[arg(long, default_value_t = 1)]
    start: u16,

    #[arg(long, default_value_t = 1024)]
    end: u16,

    #[arg(long, default_value_t = 500)]
    workers: usize,

    /// timeout ms
    #[arg(long, default_value_t = 300)]
    timeout: u64,

    #[arg(long)]
    json: bool,

    /// retries on timeout
    #[arg(long, default_value_t = 1)]
    retries: usize,

    /// enable adaptive timeout based on RTT probing
    #[arg(long, default_value_t = true)]
    adaptive: bool,

    /// optimize settings for public internet targets
    #[arg(long, default_value_t = false)]
    fast_public: bool,
}

#[derive(Serialize)]
struct ResultRec {
    port: u16,
    status: &'static str,
    banner: Option<String>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = Args::parse();
    let started = std::time::Instant::now();

    if args.end < args.start {
        eprintln!("end must be >= start");
        std::process::exit(2);
    }

    // Resolve host to first IP: use ToSocketAddrs
    let ip = match (args.host.as_str(), 0).to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(s) => s.ip().to_string(),
            None => {
                eprintln!("failed to resolve host");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("resolve error: {}", e);
            std::process::exit(1);
        }
    };

    // Derive dial timeout: optionally adaptive based on quick probes
    let base_timeout = Duration::from_millis(args.timeout);
    let mut timeout_dur = if args.adaptive && !args.fast_public { estimate_timeout(&ip, base_timeout) } else { base_timeout };
    let mut workers = args.workers;
    let mut retries = args.retries;
    if args.fast_public {
        if timeout_dur > Duration::from_millis(80) { timeout_dur = Duration::from_millis(80); }
        if retries > 0 { retries = 0; }
        if workers < 2000 { workers = 2000; }
    }

    // Wrap semaphore in Arc so it can be cheaply cloned between tasks
    let sem = Arc::new(Semaphore::new(workers));
    let mut handles = Vec::new();

    for port in args.start..=args.end {
        let sem_clone = Arc::clone(&sem);
        let ip_cloned = ip.clone();
        let timeout_dur = timeout_dur.clone();

        let handle = tokio::spawn(async move {
            // Acquire an owned permit; it releases automatically when dropped.
            let _permit = sem_clone.acquire_owned().await.unwrap();
            let addr = format!("{}:{}", ip_cloned, port);

            // attempt connect with timeout + retries on timeout
            let attempts = retries + 1;
            for try_idx in 0..attempts {
                match timeout(timeout_dur, TcpStream::connect(&addr)).await {
                    Ok(Ok(mut stream)) => {
                        let mut buf = [0u8; 256];
                        let banner = match timeout(Duration::from_millis(50), stream.read(&mut buf)).await {
                            Ok(Ok(n)) if n > 0 => Some(String::from_utf8_lossy(&buf[..n]).to_string()),
                            _ => None,
                        };
                        return ResultRec { port, status: "open", banner };
                    }
                    Ok(Err(_)) => break,        // immediate refused -> closed
                    Err(_) if try_idx + 1 < attempts => continue, // timeout -> retry
                    Err(_) => break,
                }
            }
            ResultRec { port, status: "closed", banner: None }
        });

        handles.push(handle);
    }

    let mut results = Vec::with_capacity((args.end - args.start + 1) as usize);
    for h in handles {
        if let Ok(r) = h.await {
            results.push(r);
        }
    }

    results.sort_by_key(|r| r.port);
    let mut open: Vec<_> = results.into_iter().filter(|r| r.status == "open").collect();

    // Fallback probe for common ports with a longer timeout if nothing found
    if open.is_empty() {
        let common = [22u16, 80u16];
        for &p in &common {
            let addr = format!("{}:{}", args.host, p);
            if let Ok(Ok(mut stream)) = timeout(Duration::from_millis(1000), TcpStream::connect(&addr)).await {
                let mut buf = [0u8; 256];
                let banner = match timeout(Duration::from_millis(100), stream.read(&mut buf)).await {
                    Ok(Ok(n)) if n > 0 => Some(String::from_utf8_lossy(&buf[..n]).to_string()),
                    _ => None,
                };
                open.push(ResultRec { port: p, status: "open", banner });
            }
        }
    }

    let open_len = open.len();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&open).unwrap());
        return;
    }

    if open.is_empty() {
        println!("No open ports found on {} in range {}-{}", args.host, args.start, args.end);
        return;
    }
    println!("Open ports on {}:", args.host);
    for r in open {
        if let Some(b) = r.banner {
            println!("{} - {} (banner: {:.80})", r.port, r.status, b);
        } else {
            println!("{} - {}", r.port, r.status);
        }
    }

    // summary similar to nmap
    let elapsed = started.elapsed().as_secs_f64();
    let mut elapsed_safe = elapsed;
    if elapsed_safe <= 0.0 { elapsed_safe = 1e-9; }
    let total_ports = (args.end - args.start + 1) as f64;
    let rate = total_ports / elapsed_safe;
    println!(
        "\nScanned {} ports in {:.2} seconds ({:.1} ports/sec). Open: {}",
        total_ports as u16,
        elapsed,
        rate,
        open_len
    );
}
