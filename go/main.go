// Save as main.go
package main

import (
    "encoding/json"
    "flag"
    "fmt"
    "net"
    "os"
    "sort"
    "strconv"
    "sync"
    "time"
)

type Result struct {
	Port   int    `json:"port"`
	Status string `json:"status"`
	Banner string `json:"banner,omitempty"`
}

func worker(ip string, ports <-chan int, results chan<- Result, wg *sync.WaitGroup, timeout time.Duration, retries int) {
    defer wg.Done()
    for p := range ports {
        address := net.JoinHostPort(ip, strconv.Itoa(p))

        var conn net.Conn
        var err error
        attempts := retries + 1
        for try := 0; try < attempts; try++ {
            conn, err = net.DialTimeout("tcp", address, timeout)
            if err == nil {
                break
            }
            if ne, ok := err.(net.Error); ok && ne.Timeout() && try+1 < attempts {
                // retry on timeouts
                continue
            }
            break
        }
        if err != nil {
            results <- Result{Port: p, Status: "closed"}
            continue
        }

        _ = conn.SetReadDeadline(time.Now().Add(200 * time.Millisecond))
        buf := make([]byte, 256)
        n, _ := conn.Read(buf)
        banner := ""
        if n > 0 {
            banner = string(buf[:n])
        }
        conn.Close()
        results <- Result{Port: p, Status: "open", Banner: banner}
    }
}

// estimateTimeout derives a per-host timeout by probing common ports quickly
func estimateTimeout(ip string, maxTimeout time.Duration) time.Duration {
    samples := []int{22, 80, 443, 53, 25}
    var durations []time.Duration
    for _, sp := range samples {
        addr := net.JoinHostPort(ip, strconv.Itoa(sp))
        start := time.Now()
        _ , err := net.DialTimeout("tcp", addr, 500*time.Millisecond)
        d := time.Since(start)
        // whether success or error, use elapsed as RTT-ish measure
        if err == nil {
            // close immediately if connected
            // ignore error from Close
            // conn.Close handled via short lifetime above (not stored)
        }
        durations = append(durations, d)
    }
    // pick median
    sort.Slice(durations, func(i, j int) bool { return durations[i] < durations[j] })
    median := durations[len(durations)/2]
    if median < 50*time.Millisecond {
        median = 50 * time.Millisecond
    }
    derived := 3 * median
    if derived < 150*time.Millisecond {
        derived = 150 * time.Millisecond
    }
    if derived > maxTimeout {
        derived = maxTimeout
    }
    return derived
}

func main() {
	var host string
	var start, end int
	var workers int
	var timeoutMs int
	var outJSON bool
    var retries int
    var adaptive bool

	flag.StringVar(&host, "host", "", "target host (ip or domain)")
	flag.IntVar(&start, "start", 1, "start port")
	flag.IntVar(&end, "end", 1024, "end port")
	flag.IntVar(&workers, "workers", 500, "max concurrent dial attempts")
	flag.IntVar(&timeoutMs, "timeout", 300, "connect timeout in ms")
	flag.BoolVar(&outJSON, "json", false, "output results as JSON")
    flag.IntVar(&retries, "retries", 1, "number of retries on timeout")
    flag.BoolVar(&adaptive, "adaptive", true, "auto-tune timeout based on RTT")
    flag.Parse()

	if host == "" {
		fmt.Fprintln(os.Stderr, "host is required. Example: --host scanme.nmap.org")
		os.Exit(2)
	}
	if start < 1 {
		start = 1
	}
	if end > 65535 {
		end = 65535
	}
	if end < start {
		fmt.Fprintln(os.Stderr, "end must be >= start")
		os.Exit(2)
	}
	// mark start time for summary
	begin := time.Now()

    ips, err := net.LookupHost(host)
	if err != nil || len(ips) == 0 {
		fmt.Fprintf(os.Stderr, "failed to resolve host: %v\n", err)
		os.Exit(1)
	}
	ip := ips[0]

    portsCh := make(chan int, 1000)
    resultsCh := make(chan Result, 1000)

    var wg sync.WaitGroup

    // Spawn worker goroutines (limited by workers flag)
	numWorkers := workers
	if numWorkers < 1 {
		numWorkers = 100
	}
    // compute dial timeout
    baseTimeout := time.Duration(timeoutMs) * time.Millisecond
    dialTimeout := baseTimeout
    if adaptive {
        dialTimeout = estimateTimeout(ip, baseTimeout)
    }
    for i := 0; i < numWorkers; i++ {
		wg.Add(1)
        go worker(ip, portsCh, resultsCh, &wg, dialTimeout, retries)
	}

	// feed ports
	go func() {
		for p := start; p <= end; p++ {
			portsCh <- p
		}
		close(portsCh)
	}()

    // Collect results concurrently to avoid blocking workers on full channel
    var resList []Result
    var collectWg sync.WaitGroup
    collectWg.Add(1)
    go func() {
        defer collectWg.Done()
        for r := range resultsCh {
            resList = append(resList, r)
        }
    }()

    // Wait for workers to finish and close results
    wg.Wait()
    close(resultsCh)
    collectWg.Wait()

	// sort by port
	sort.Slice(resList, func(i, j int) bool { return resList[i].Port < resList[j].Port })

	// filter open ports for printing
	var open []Result
	for _, r := range resList {
		if r.Status == "open" {
			open = append(open, r)
		}
	}

	if outJSON {
		enc := json.NewEncoder(os.Stdout)
		enc.SetIndent("", "  ")
		_ = enc.Encode(open)
		return
	}

	if len(open) == 0 {
		fmt.Printf("No open ports found on %s (%s) in range %d-%d\n", host, ip, start, end)
		return
	}
	fmt.Printf("Open ports on %s (%s):\n", host, ip)
	for _, o := range open {
		if o.Banner != "" {
			fmt.Printf("%d - %s (banner: %.80s)\n", o.Port, o.Status, o.Banner)
		} else {
			fmt.Printf("%d - %s\n", o.Port, o.Status)
		}
	}

	// summary similar to nmap
	elapsed := time.Since(begin).Seconds()
	if elapsed < 1e-9 {
		elapsed = 1e-9
	}
	totalPorts := (end - start) + 1
	rate := float64(totalPorts) / elapsed
	fmt.Printf("\nScanned %d ports in %.2f seconds (%.1f ports/sec). Open: %d\n", totalPorts, elapsed, rate, len(open))
}
