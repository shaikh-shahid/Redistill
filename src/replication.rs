#![allow(dead_code)] // wired into the server in later replication tasks; tighten then

//! Asynchronous primary→replica replication.
//!
//! A primary feeds every successful write into a broadcast channel and tracks
//! a monotonic byte offset. Replicas receive a consistent snapshot followed by
//! the live command stream and apply it through the normal command path.

use crate::persistence::dump_store_to_bytes;
use crate::store::ShardedStore;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Primary,
    Replica,
}

/// State describing the primary this node replicates from (replica role only).
#[derive(Debug, Clone)]
pub struct MasterInfo {
    pub host: String,
    pub port: u16,
    /// true once the initial sync completed and the stream is live.
    pub link_up: bool,
}

/// A single frame on the replication stream: (end_offset, resp_bytes).
pub type Frame = (u64, Bytes);

pub struct Replication {
    role: RwLock<Role>,
    master: RwLock<Option<MasterInfo>>,
    replid: RwLock<String>,
    offset: AtomicU64,
    /// Guards (offset increment + broadcast send) so frame offsets are
    /// monotonic and consistent with broadcast order.
    feed_lock: Mutex<()>,
    sender: broadcast::Sender<Frame>,
    /// Held shared by every command; held exclusively during a full-sync cut so
    /// the snapshot is quiescent and aligns with a single offset.
    pub barrier: RwLock<()>,
    /// Count of currently connected replicas (for INFO / metrics).
    replica_count: AtomicU64,
    /// Bumped whenever the master target changes (set_master / promote).
    /// A replica_client task checks this to detect supersession.
    gen_tx: watch::Sender<u64>,
}

impl Replication {
    pub fn new(backlog: usize) -> Self {
        let (sender, _rx) = broadcast::channel(backlog.max(16));
        let (gen_tx, _gen_rx) = watch::channel(0u64);
        Self {
            role: RwLock::new(Role::Primary),
            master: RwLock::new(None),
            replid: RwLock::new(gen_replid()),
            offset: AtomicU64::new(0),
            feed_lock: Mutex::new(()),
            sender,
            barrier: RwLock::new(()),
            replica_count: AtomicU64::new(0),
            gen_tx,
        }
    }

    pub fn role(&self) -> Role {
        *self.role.read()
    }
    pub fn offset(&self) -> u64 {
        self.offset.load(Ordering::Relaxed)
    }
    pub fn replid(&self) -> String {
        self.replid.read().clone()
    }
    pub fn master(&self) -> Option<MasterInfo> {
        self.master.read().clone()
    }
    pub fn replica_count(&self) -> u64 {
        self.replica_count.load(Ordering::Relaxed)
    }
    pub fn incr_replicas(&self) {
        self.replica_count.fetch_add(1, Ordering::Relaxed);
    }
    pub fn decr_replicas(&self) {
        self.replica_count.fetch_sub(1, Ordering::Relaxed);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.sender.subscribe()
    }

    /// Current replica "generation"; bumped whenever the master target changes
    /// (set_master / promote). A replica_client task is tied to the generation
    /// in effect when it was spawned.
    pub fn current_gen(&self) -> u64 {
        *self.gen_tx.borrow()
    }
    fn gen_subscribe(&self) -> watch::Receiver<u64> {
        self.gen_tx.subscribe()
    }
    fn bump_gen(&self) -> u64 {
        let mut new = 0;
        self.gen_tx.send_modify(|g| {
            *g += 1;
            new = *g;
        });
        new
    }

    /// Feed a write command to all replicas. Cheap when there are no subscribers.
    /// Called from the synchronous command path.
    pub fn feed(&self, command: &[Bytes]) {
        if *self.role.read() != Role::Primary {
            return;
        }
        let frame = encode_command(command);
        let _g = self.feed_lock.lock();
        let new_off =
            self.offset.fetch_add(frame.len() as u64, Ordering::Relaxed) + frame.len() as u64;
        // Ignore send errors: zero subscribers is the common case.
        let _ = self.sender.send((new_off, frame));
    }

    /// Become a replica of host:port. Sets role + master info; the actual
    /// connection is driven by the replica client task. Returns the new
    /// generation so the caller can tie the spawned task to it.
    pub fn set_master(&self, host: String, port: u16) -> u64 {
        *self.role.write() = Role::Replica;
        *self.master.write() = Some(MasterInfo {
            host,
            port,
            link_up: false,
        });
        self.bump_gen()
    }

    pub fn set_link_up(&self, up: bool) {
        if let Some(m) = self.master.write().as_mut() {
            m.link_up = up;
        }
    }

    /// Promote to primary (`REPLICAOF NO ONE`). Fresh replid, keep offset.
    /// Bumps the generation to cancel any running replica_client task.
    pub fn promote(&self) {
        *self.role.write() = Role::Primary;
        *self.master.write() = None;
        *self.replid.write() = gen_replid();
        self.bump_gen();
    }
}

/// Generate a 40-char lowercase hex run id.
pub fn gen_replid() -> String {
    let mut s = String::with_capacity(40);
    for _ in 0..40 {
        let nibble = fastrand::u8(0..16);
        s.push(char::from_digit(nibble as u32, 16).unwrap());
    }
    s
}

/// Serialize a command as a RESP array frame (same wire shape as the AOF).
pub fn encode_command(command: &[Bytes]) -> Bytes {
    let mut buf = Vec::with_capacity(16 + command.iter().map(|p| p.len() + 16).sum::<usize>());
    buf.extend_from_slice(format!("*{}\r\n", command.len()).as_bytes());
    for part in command {
        buf.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
        buf.extend_from_slice(part);
        buf.extend_from_slice(b"\r\n");
    }
    Bytes::from(buf)
}

/// Serve a replica over `stream` after it issued PSYNC. Runs until the replica
/// disconnects or a write error occurs.
pub async fn serve_replica<S>(
    repl: &Replication,
    store: &ShardedStore,
    stream: &mut S,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    // --- Consistency cut: exclusive barrier => quiescent store + single offset.
    // All sync work; the guard is dropped at the end of this block, before any await.
    let (snapshot, cut_offset, replid, mut rx) = {
        let _ex = repl.barrier.write();
        let rx = repl.subscribe(); // subscribe inside the exclusive section
        let cut = repl.offset();
        let snap = dump_store_to_bytes(store).map_err(std::io::Error::other)?;
        (snap, cut, repl.replid(), rx)
    };

    // --- FULLRESYNC line.
    let line = format!("+FULLRESYNC {} {}\r\n", replid, cut_offset);
    stream.write_all(line.as_bytes()).await?;

    // --- Length-prefixed snapshot payload: "$<len>\r\n<bytes>" (no trailing CRLF).
    let hdr = format!("${}\r\n", snapshot.len());
    stream.write_all(hdr.as_bytes()).await?;
    stream.write_all(&snapshot).await?;
    stream.flush().await?;

    repl.incr_replicas();
    let result = stream_loop(&mut rx, cut_offset, stream).await;
    repl.decr_replicas();
    result
}

async fn stream_loop<S>(
    rx: &mut broadcast::Receiver<Frame>,
    cut_offset: u64,
    stream: &mut S,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    loop {
        match rx.recv().await {
            Ok((end_off, bytes)) => {
                // Discard frames already captured in the snapshot.
                if end_off <= cut_offset {
                    continue;
                }
                stream.write_all(&bytes).await?;
                stream.flush().await?;
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Replica too slow: drop it so it reconnects and full-resyncs.
                return Err(std::io::Error::other("replica lagged off the backlog"));
            }
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

/// Read one CRLF-terminated line, returned without the trailing CRLF.
async fn read_line<R: AsyncBufReadExt + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let n = r.read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Err(std::io::Error::other("eof during line read"));
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(buf)
}

/// Read a `$<len>\r\n` bulk-length header and return len.
async fn read_bulk_len<R: AsyncBufReadExt + Unpin>(r: &mut R) -> std::io::Result<usize> {
    let line = read_line(r).await?;
    if line.first() != Some(&b'$') {
        return Err(std::io::Error::other("expected $ bulk length"));
    }
    std::str::from_utf8(&line[1..])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| std::io::Error::other("bad bulk length"))
}

/// Read one RESP array command (`*N` + N bulk strings each with trailing CRLF).
/// Returns Ok(None) on a clean EOF before any bytes.
async fn read_command<R: AsyncBufReadExt + Unpin>(
    r: &mut R,
) -> std::io::Result<Option<Vec<Bytes>>> {
    let mut header = Vec::new();
    let n = r.read_until(b'\n', &mut header).await?;
    if n == 0 {
        return Ok(None);
    }
    if header.first() != Some(&b'*') {
        return Err(std::io::Error::other("expected * array header"));
    }
    while header.last() == Some(&b'\n') || header.last() == Some(&b'\r') {
        header.pop();
    }
    let count: usize = std::str::from_utf8(&header[1..])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| std::io::Error::other("bad array len"))?;
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_bulk_len(r).await?;
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf).await?;
        let mut crlf = [0u8; 2];
        r.read_exact(&mut crlf).await?; // consume trailing CRLF
        parts.push(Bytes::from(buf));
    }
    Ok(Some(parts))
}

/// Run the replica side forever: (re)connect to `host:port`, full-sync, then
/// apply the live stream. Stops when superseded by a newer REPLICAOF/promote
/// (detected via the generation watch channel).
#[allow(clippy::too_many_arguments)]
pub async fn replica_client(
    repl: std::sync::Arc<Replication>,
    host: String,
    port: u16,
    masterauth: String,
    listening_port: u16,
    my_gen: u64,
    clear: impl Fn() + Send + Sync,
    load: impl Fn(&[u8]) + Send + Sync,
    apply: impl Fn(Vec<Bytes>) + Send + Sync,
) {
    let mut gen_rx = repl.gen_subscribe();
    let mut backoff = Duration::from_millis(200);
    loop {
        // Stop if a newer REPLICAOF/promote superseded this task.
        if repl.current_gen() != my_gen {
            return;
        }
        let sync = connect_and_sync(
            &repl,
            &host,
            port,
            &masterauth,
            listening_port,
            &clear,
            &load,
            &apply,
        );
        tokio::select! {
            res = sync => {
                match res {
                    Ok(()) => eprintln!("replication stream closed by master {}:{}", host, port),
                    Err(e) => eprintln!("replica sync failed ({}:{}): {}; retrying", host, port, e),
                }
            }
            _ = gen_rx.changed() => {
                // Superseded mid-connection: drop the socket (the `sync` future is
                // cancelled here) and exit so we never apply concurrently with the
                // new task.
                return;
            }
        }
        repl.set_link_up(false);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

#[allow(clippy::too_many_arguments)]
async fn connect_and_sync(
    repl: &Replication,
    host: &str,
    port: u16,
    masterauth: &str,
    listening_port: u16,
    clear: &(impl Fn() + Send + Sync),
    load: &(impl Fn(&[u8]) + Send + Sync),
    apply: &(impl Fn(Vec<Bytes>) + Send + Sync),
) -> std::io::Result<()> {
    let stream = TcpStream::connect((host, port)).await?;
    let (rd, mut wr) = stream.into_split();
    let mut reader = BufReader::new(rd);

    // Handshake.
    wr.write_all(b"*1\r\n$4\r\nPING\r\n").await?;
    read_line(&mut reader).await?; // +PONG

    if !masterauth.is_empty() {
        let auth = encode_command(&[
            Bytes::from_static(b"AUTH"),
            Bytes::from(masterauth.as_bytes().to_vec()),
        ]);
        wr.write_all(&auth).await?;
        read_line(&mut reader).await?; // +OK
    }

    let lport = encode_command(&[
        Bytes::from_static(b"REPLCONF"),
        Bytes::from_static(b"listening-port"),
        Bytes::from(listening_port.to_string().into_bytes()),
    ]);
    wr.write_all(&lport).await?;
    read_line(&mut reader).await?; // +OK

    wr.write_all(b"*3\r\n$5\r\nPSYNC\r\n$1\r\n?\r\n$2\r\n-1\r\n")
        .await?;
    let _fullresync = read_line(&mut reader).await?; // +FULLRESYNC <replid> <offset>

    // Snapshot: $<len>\r\n<bytes>  (no trailing CRLF).
    let len = read_bulk_len(&mut reader).await?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    clear();
    load(&buf);
    repl.set_link_up(true);
    eprintln!(
        "replication: full sync complete from {}:{} ({} bytes)",
        host, port, len
    );

    // Live stream.
    loop {
        match read_command(&mut reader).await? {
            Some(cmd) => apply(cmd),
            None => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replid_is_40_hex_chars() {
        let id = gen_replid();
        assert_eq!(id.len(), 40);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn encode_command_produces_resp_array() {
        let cmd = vec![
            Bytes::from_static(b"SET"),
            Bytes::from_static(b"k"),
            Bytes::from_static(b"v"),
        ];
        let out = encode_command(&cmd);
        assert_eq!(&out[..], b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$1\r\nv\r\n");
    }

    #[test]
    fn new_replication_is_primary_with_zero_offset() {
        let r = Replication::new(16);
        assert!(matches!(r.role(), Role::Primary));
        assert_eq!(r.offset(), 0);
        assert_eq!(r.replid().len(), 40);
    }

    #[test]
    fn feed_advances_offset() {
        let r = Replication::new(16);
        // Subscribe so the frame is actually buffered (and to mimic a replica).
        let _rx = r.subscribe();
        r.feed(&[
            Bytes::from_static(b"SET"),
            Bytes::from_static(b"k"),
            Bytes::from_static(b"v"),
        ]);
        assert!(r.offset() > 0);
    }
}
