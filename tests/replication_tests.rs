// Replication integration tests - Task 1: in-memory snapshot dump/load
use bytes::Bytes;
use redistill::ShardedStore;
use redistill::persistence::{dump_store_to_bytes, load_store_from_bytes};

fn now() -> u64 {
    redistill::get_timestamp()
}

#[test]
fn dump_and_load_roundtrip_strings_and_hashes() {
    let src = ShardedStore::new(16);
    let t = now();
    src.set(
        Bytes::from_static(b"k1"),
        Bytes::from_static(b"v1"),
        None,
        t,
    );
    src.set(
        Bytes::from_static(b"k2"),
        Bytes::from_static(b"v2"),
        Some(t + 100),
        t,
    );
    src.hset(
        Bytes::from_static(b"h1"),
        &[(Bytes::from_static(b"f1"), Bytes::from_static(b"hv1"))],
        t,
    )
    .unwrap();

    let bytes = dump_store_to_bytes(&src).expect("dump");

    let dst = ShardedStore::new(16);
    let count = load_store_from_bytes(&dst, &bytes).expect("load");

    assert_eq!(count, 3);
    assert_eq!(dst.get(b"k1", now()), Some(Bytes::from_static(b"v1")));
    assert_eq!(dst.get(b"k2", now()), Some(Bytes::from_static(b"v2")));
    let hv = dst.hget(b"h1", b"f1", now()).unwrap();
    assert_eq!(hv, Some(Bytes::from_static(b"hv1")));
}
