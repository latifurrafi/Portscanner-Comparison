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

    // Wrap semaphore in Arc so it can be cheaply cloned between tasks
    let sem = Arc::new(Semaphore::new(args.workers));
    let mut handles = Vec::new();
    let timeout_dur = Duration::from_millis(args.timeout);

    for port in args.start..=args.end {
        let sem_clone = Arc::clone(&sem);
        let ip_cloned = ip.clone();
        let timeout_dur = timeout_dur.clone();

        let handle = tokio::spawn(async move {
            // Acquire an owned permit; it releases automatically when dropped.
            let _permit = sem_clone.acquire_owned().await.unwrap();
            let addr = format!("{}:{}", ip_cloned, port);

            // attempt connect with timeout
            match timeout(timeout_dur, TcpStream::connect(&addr)).await {
                Ok(Ok(mut stream)) => {
                    // try small banner read (short deadline)
                    let mut buf = [0u8; 256];
                    let banner = match timeout(Duration::from_millis(200), stream.read(&mut buf)).await {
                        Ok(Ok(n)) if n > 0 => Some(String::from_utf8_lossy(&buf[..n]).to_string()),
                        _ => None,
                    };
                    ResultRec { port, status: "open", banner }
                }
                _ => {
                    ResultRec { port, status: "closed", banner: None }
                }
            }
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
    let open: Vec<_> = results.into_iter().filter(|r| r.status == "open").collect();
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
