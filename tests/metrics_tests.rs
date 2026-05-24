// /metrics endpoint integration test.
//
// Starts a real server, issues a handful of commands, scrapes the Prometheus
// endpoint, and asserts (a) the response is well-formed text exposition and
// (b) it contains the counters and gauges that PR-4 added.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_redistill");

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

fn wait_for_port(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server on {} never came up", port);
}

fn send_and_read_reply(conn: &mut TcpStream, req: &[u8]) -> Vec<u8> {
    conn.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    conn.write_all(req).unwrap();
    let mut buf = vec![0u8; 256];
    let n = conn.read(&mut buf).unwrap();
    buf.truncate(n);
    buf
}

struct Guard(Option<Child>);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Minimal blocking HTTP GET that doesn't pull in another dep. Only used in
/// this single test so we don't reach for hyper here.
fn http_get(port: u16, path: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        path, port
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();

    let text = String::from_utf8_lossy(&buf).into_owned();
    // Parse first line for status code, find header/body boundary at \r\n\r\n.
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (headers, body) = match text.find("\r\n\r\n") {
        Some(idx) => (text[..idx].to_string(), text[idx + 4..].to_string()),
        None => (text.clone(), String::new()),
    };
    (status, headers, body)
}

#[test]
fn test_metrics_endpoint_exposes_prometheus_text() {
    let redis_port = free_port();
    let http_port = free_port();

    let child = Command::new(BIN)
        .env("REDIS_PORT", redis_port.to_string())
        .env("REDIS_BIND", "127.0.0.1")
        .env("REDIS_HEALTH_CHECK_PORT", http_port.to_string())
        .env("REDIS_PERSISTENCE_ENABLED", "false")
        .env("REDIS_AOF_ENABLED", "false")
        // Per-command instrumentation is opt-in for performance reasons.
        // Turn it on so the assertions below find the per-label series.
        .env("REDIS_METRICS_COMMAND_COUNTER", "true")
        .env("REDIS_METRICS_COMMAND_HISTOGRAM", "true")
        .env("REDIS_SHUTDOWN_GRACE_SECS", "5")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let _guard = Guard(Some(child));

    wait_for_port(redis_port, Duration::from_secs(5));
    wait_for_port(http_port, Duration::from_secs(5));

    // Issue a few commands so the per-command counter records something.
    {
        let mut conn = TcpStream::connect(("127.0.0.1", redis_port)).unwrap();
        assert_eq!(
            &send_and_read_reply(&mut conn, b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\nbar\r\n"),
            b"+OK\r\n"
        );
        assert_eq!(
            &send_and_read_reply(&mut conn, b"*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n"),
            b"$3\r\nbar\r\n"
        );
        assert_eq!(
            &send_and_read_reply(&mut conn, b"*1\r\n$4\r\nPING\r\n"),
            b"+PONG\r\n"
        );
    }

    // Scrape.
    let (status, headers, body) = http_get(http_port, "/metrics");
    assert_eq!(status, 200, "wrong status; headers: {}", headers);
    assert!(
        headers.to_lowercase().contains("text/plain"),
        "wrong content-type; headers: {}",
        headers
    );

    // Spot-check exposition format and that our specific metrics show up.
    for expected in [
        "# HELP redistill_commands_total",
        "# TYPE redistill_commands_total counter",
        "redistill_commands_total{cmd=\"set\"}",
        "redistill_commands_total{cmd=\"get\"}",
        "redistill_commands_total{cmd=\"ping\"}",
        "redistill_command_duration_seconds",
        "redistill_connections_active",
        "redistill_memory_used_bytes",
        "redistill_keys_total",
        "redistill_aof_size_bytes",
        "redistill_build_info",
    ] {
        assert!(
            body.contains(expected),
            "missing metric `{}` in /metrics body:\n{}",
            expected,
            body
        );
    }

    // The set counter should be >= 1 since we issued SET.
    let set_line = body
        .lines()
        .find(|l| l.starts_with("redistill_commands_total{cmd=\"set\"}"))
        .expect("set counter line");
    let value: u64 = set_line.split_whitespace().last().unwrap().parse().unwrap();
    assert!(value >= 1, "expected at least one SET, got {}", value);

    // /health still works.
    let (status_h, _h, body_h) = http_get(http_port, "/health");
    assert_eq!(status_h, 200);
    assert!(
        body_h.contains("\"status\":\"healthy\""),
        "body: {}",
        body_h
    );

    // Unknown path 404s.
    let (status_n, _, _) = http_get(http_port, "/does-not-exist");
    assert_eq!(status_n, 404);
}
