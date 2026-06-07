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

struct Guard(Option<Child>);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn base_cmd(port: u16) -> Command {
    let mut c = Command::new(BIN);
    c.env("REDIS_PORT", port.to_string())
        .env("REDIS_BIND", "127.0.0.1")
        .env("REDIS_HEALTH_CHECK_PORT", "0")
        .env("REDIS_PERSISTENCE_ENABLED", "false")
        .env("REDIS_AOF_ENABLED", "false")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

fn spawn_primary(port: u16) -> Child {
    base_cmd(port).spawn().expect("spawn primary")
}

fn spawn_replica(port: u16, master_port: u16) -> Child {
    base_cmd(port)
        .env("REDIS_REPLICAOF", format!("127.0.0.1:{}", master_port))
        .spawn()
        .expect("spawn replica")
}

fn encode(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = format!("*{}\r\n", parts.len()).into_bytes();
    for p in parts {
        v.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
        v.extend_from_slice(p);
        v.extend_from_slice(b"\r\n");
    }
    v
}

fn req(conn: &mut TcpStream, parts: &[&[u8]]) -> Vec<u8> {
    conn.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    conn.write_all(&encode(parts)).unwrap();
    let mut buf = vec![0u8; 256];
    let n = conn.read(&mut buf).unwrap();
    buf.truncate(n);
    buf
}

/// Poll GET on a fresh connection until it returns the expected bulk string.
fn wait_for_value(port: u16, key: &[u8], expected: &[u8], timeout: Duration) {
    let want = {
        let mut v = format!("${}\r\n", expected.len()).into_bytes();
        v.extend_from_slice(expected);
        v.extend_from_slice(b"\r\n");
        v
    };
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(mut c) = TcpStream::connect(("127.0.0.1", port))
            && req(&mut c, &[b"GET", key]) == want
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "replica never converged on key {:?}",
        String::from_utf8_lossy(key)
    );
}

#[test]
fn replica_full_syncs_existing_data() {
    let mport = free_port();
    let _mp = Guard(Some(spawn_primary(mport)));
    wait_for_port(mport, Duration::from_secs(5));

    let mut m = TcpStream::connect(("127.0.0.1", mport)).unwrap();
    assert_eq!(req(&mut m, &[b"SET", b"pre", b"loaded"]), b"+OK\r\n");

    let rport = free_port();
    let _rp = Guard(Some(spawn_replica(rport, mport)));
    wait_for_port(rport, Duration::from_secs(5));

    wait_for_value(rport, b"pre", b"loaded", Duration::from_secs(5));
}

#[test]
fn writes_propagate_to_replica() {
    let mport = free_port();
    let _mp = Guard(Some(spawn_primary(mport)));
    wait_for_port(mport, Duration::from_secs(5));
    let rport = free_port();
    let _rp = Guard(Some(spawn_replica(rport, mport)));
    wait_for_port(rport, Duration::from_secs(5));

    let mut m = TcpStream::connect(("127.0.0.1", mport)).unwrap();
    assert_eq!(req(&mut m, &[b"SET", b"live", b"yes"]), b"+OK\r\n");
    wait_for_value(rport, b"live", b"yes", Duration::from_secs(5));
}

#[test]
fn replica_rejects_writes() {
    let mport = free_port();
    let _mp = Guard(Some(spawn_primary(mport)));
    wait_for_port(mport, Duration::from_secs(5));
    let rport = free_port();
    let _rp = Guard(Some(spawn_replica(rport, mport)));
    wait_for_port(rport, Duration::from_secs(5));
    // Give the replica a moment to establish replica role.
    std::thread::sleep(Duration::from_millis(800));

    let mut r = TcpStream::connect(("127.0.0.1", rport)).unwrap();
    let reply = req(&mut r, &[b"SET", b"x", b"1"]);
    assert!(
        reply.starts_with(b"-READONLY"),
        "expected READONLY, got {:?}",
        String::from_utf8_lossy(&reply)
    );
}

#[test]
fn incrby_applies_exactly_once() {
    let mport = free_port();
    let _mp = Guard(Some(spawn_primary(mport)));
    wait_for_port(mport, Duration::from_secs(5));

    let mut m = TcpStream::connect(("127.0.0.1", mport)).unwrap();
    assert_eq!(req(&mut m, &[b"SET", b"counter", b"0"]), b"+OK\r\n");

    let rport = free_port();
    let _rp = Guard(Some(spawn_replica(rport, mport)));
    wait_for_port(rport, Duration::from_secs(5));

    for _ in 0..50 {
        let reply = req(&mut m, &[b"INCRBY", b"counter", b"1"]);
        assert!(
            reply.starts_with(b":"),
            "got {:?}",
            String::from_utf8_lossy(&reply)
        );
    }
    // Replica must converge to exactly 50 — proves the snapshot/stream cut
    // applied every INCRBY exactly once (no gap, no double-apply).
    wait_for_value(rport, b"counter", b"50", Duration::from_secs(5));
}

#[test]
fn promoted_replica_accepts_writes() {
    let mport = free_port();
    let _mp = Guard(Some(spawn_primary(mport)));
    wait_for_port(mport, Duration::from_secs(5));
    let rport = free_port();
    let _rp = Guard(Some(spawn_replica(rport, mport)));
    wait_for_port(rport, Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(800));

    let mut r = TcpStream::connect(("127.0.0.1", rport)).unwrap();
    assert!(req(&mut r, &[b"SET", b"x", b"1"]).starts_with(b"-READONLY"));
    assert_eq!(req(&mut r, &[b"REPLICAOF", b"NO", b"ONE"]), b"+OK\r\n");
    assert_eq!(req(&mut r, &[b"SET", b"x", b"1"]), b"+OK\r\n");
}
