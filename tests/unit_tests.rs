// Unit tests for Redistill's core storage and functionality
// These tests verify the ShardedStore, eviction policies, and helper functions
//
// Note: This file is in the tests/ directory to keep tests separate from main.rs
// while still testing internal implementation details through the lib.rs interface.

use bytes::Bytes;
use redistill::*;
use std::sync::Arc;
use std::thread;

// Test helper: Create a test store
fn create_test_store() -> ShardedStore {
    ShardedStore::new(16) // Smaller for tests
}

// Test helper: Get current timestamp
fn now() -> u64 {
    get_timestamp()
}

// ==================== Basic Storage Tests ====================

#[test]
fn test_sharded_store_set_and_get() {
    let store = create_test_store();
    let key = Bytes::from("test_key");
    let value = Bytes::from("test_value");
    
    store.set(key.clone(), value.clone(), None, now());
    
    let result = store.get(&key, now());
    assert!(result.is_some());
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_store_get_nonexistent() {
    let store = create_test_store();
    let key = Bytes::from("nonexistent");
    
    let result = store.get(&key, now());
    assert!(result.is_none());
}

#[test]
fn test_store_delete() {
    let store = create_test_store();
    let key = Bytes::from("delete_me");
    let value = Bytes::from("value");
    
    store.set(key.clone(), value, None, now());
    
    let count = store.delete(&[key.clone()]);
    assert_eq!(count, 1);
    
    let result = store.get(&key, now());
    assert!(result.is_none());
}

#[test]
fn test_store_delete_multiple() {
    let store = create_test_store();
    let keys: Vec<Bytes> = (0..5)
        .map(|i| Bytes::from(format!("key{}", i)))
        .collect();
    
    // Set all keys
    for key in &keys {
        store.set(key.clone(), Bytes::from("value"), None, now());
    }
    
    // Delete all
    let count = store.delete(&keys);
    assert_eq!(count, 5);
    
    // Verify all deleted
    for key in &keys {
        assert!(store.get(key, now()).is_none());
    }
}

#[test]
fn test_store_overwrite() {
    let store = create_test_store();
    let key = Bytes::from("key");
    let value1 = Bytes::from("value1");
    let value2 = Bytes::from("value2");
    
    store.set(key.clone(), value1, None, now());
    store.set(key.clone(), value2.clone(), None, now());
    
    let result = store.get(&key, now()).unwrap();
    assert_eq!(result, value2);
}

#[test]
fn test_store_exists() {
    let store = create_test_store();
    let key1 = Bytes::from("exists1");
    let key2 = Bytes::from("exists2");
    let key3 = Bytes::from("not_exists");
    
    store.set(key1.clone(), Bytes::from("v1"), None, now());
    store.set(key2.clone(), Bytes::from("v2"), None, now());
    
    let count = store.exists(&[key1, key2, key3], now());
    assert_eq!(count, 2);
}

#[test]
fn test_store_len() {
    let store = create_test_store();
    
    assert_eq!(store.len(), 0);
    
    for i in 0..10 {
        store.set(
            Bytes::from(format!("key{}", i)),
            Bytes::from("value"),
            None,
            now()
        );
    }
    
    assert_eq!(store.len(), 10);
}

#[test]
fn test_store_flush() {
    let store = create_test_store();
    
    // Add data
    for i in 0..100 {
        store.set(
            Bytes::from(format!("key{}", i)),
            Bytes::from("value"),
            None,
            now()
        );
    }
    
    assert_eq!(store.len(), 100);
    
    store.clear();
    
    assert_eq!(store.len(), 0);
}

// ==================== TTL/Expiration Tests ====================

#[test]
fn test_store_ttl_expiration() {
    let store = create_test_store();
    let key = Bytes::from("expiring_key");
    let value = Bytes::from("value");
    
    // Set with 1 second TTL
    let timestamp = now();
    store.set(key.clone(), value.clone(), Some(1), timestamp);
    
    // Should exist immediately
    assert!(store.get(&key, timestamp).is_some());
    
    // Should be expired after TTL
    let future = timestamp + 2;
    assert!(store.get(&key, future).is_none());
}

#[test]
fn test_ttl_edge_cases() {
    let store = create_test_store();
    let key = Bytes::from("ttl_test");
    let value = Bytes::from("value");
    
    // Set with 0 TTL (should expire immediately)
    let timestamp = now();
    store.set(key.clone(), value.clone(), Some(0), timestamp);
    
    // Should be expired
    assert!(store.get(&key, timestamp).is_none());
}

#[test]
fn test_multiple_ttl_keys() {
    let store = create_test_store();
    let timestamp = now();
    
    // Set multiple keys with different TTLs
    store.set(Bytes::from("ttl1"), Bytes::from("v1"), Some(1), timestamp);
    store.set(Bytes::from("ttl5"), Bytes::from("v5"), Some(5), timestamp);
    store.set(Bytes::from("ttl10"), Bytes::from("v10"), Some(10), timestamp);
    
    // At t+2, ttl1 should be expired
    let future2 = timestamp + 2;
    assert!(store.get(&Bytes::from("ttl1"), future2).is_none());
    assert!(store.get(&Bytes::from("ttl5"), future2).is_some());
    assert!(store.get(&Bytes::from("ttl10"), future2).is_some());
    
    // At t+6, ttl1 and ttl5 should be expired
    let future6 = timestamp + 6;
    assert!(store.get(&Bytes::from("ttl1"), future6).is_none());
    assert!(store.get(&Bytes::from("ttl5"), future6).is_none());
    assert!(store.get(&Bytes::from("ttl10"), future6).is_some());
    
    // At t+11, all should be expired
    let future11 = timestamp + 11;
    assert!(store.get(&Bytes::from("ttl1"), future11).is_none());
    assert!(store.get(&Bytes::from("ttl5"), future11).is_none());
    assert!(store.get(&Bytes::from("ttl10"), future11).is_none());
}

#[test]
fn test_overwrite_with_ttl() {
    let store = create_test_store();
    let key = Bytes::from("key");
    let timestamp = now();
    
    // Set without TTL
    store.set(key.clone(), Bytes::from("v1"), None, timestamp);
    assert!(store.get(&key, timestamp + 100).is_some());
    
    // Overwrite with TTL
    store.set(key.clone(), Bytes::from("v2"), Some(5), timestamp);
    assert!(store.get(&key, timestamp).is_some());
    assert!(store.get(&key, timestamp + 6).is_none());
}

// ==================== Helper Functions Tests ====================

#[test]
fn test_eq_ignore_case_3() {
    assert!(eq_ignore_case_3(b"GET", b"get"));
    assert!(eq_ignore_case_3(b"set", b"set"));
    assert!(eq_ignore_case_3(b"SET", b"set"));
    assert!(eq_ignore_case_3(b"Del", b"del"));
    assert!(!eq_ignore_case_3(b"get", b"set"));
}

#[test]
fn test_eq_ignore_case_6() {
    assert!(eq_ignore_case_6(b"EXISTS", b"exists"));
    assert!(eq_ignore_case_6(b"dbsize", b"dbsize"));
    assert!(eq_ignore_case_6(b"DBSIZE", b"dbsize"));
    assert!(!eq_ignore_case_6(b"exists", b"dbsize"));
}

#[test]
fn test_entry_size_calculation() {
    let size = entry_size(10, 100);
    assert_eq!(size, 174); // 10 + 100 + 64 overhead
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(500), "500B");
    assert_eq!(format_bytes(1024), "1.00KB");
    assert_eq!(format_bytes(1024 * 1024), "1.00MB");
    assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00GB");
}

// ==================== Eviction Policy Tests ====================

#[test]
fn test_eviction_policy_from_str() {
    assert_eq!(
        EvictionPolicy::from_str("allkeys-lru"),
        EvictionPolicy::AllKeysLru
    );
    assert_eq!(
        EvictionPolicy::from_str("allkeys-random"),
        EvictionPolicy::AllKeysRandom
    );
    assert_eq!(
        EvictionPolicy::from_str("noeviction"),
        EvictionPolicy::NoEviction
    );
    // Default on unknown
    assert_eq!(
        EvictionPolicy::from_str("unknown"),
        EvictionPolicy::AllKeysLru
    );
}

#[test]
fn test_eviction_policy_as_str() {
    assert_eq!(EvictionPolicy::AllKeysLru.as_str(), "allkeys-lru");
    assert_eq!(EvictionPolicy::AllKeysRandom.as_str(), "allkeys-random");
    assert_eq!(EvictionPolicy::NoEviction.as_str(), "noeviction");
}

#[test]
fn test_memory_limit_zero_cost() {
    // When max_memory is 0, eviction should always pass
    let store = create_test_store();
    
    // This should work even with large size
    let result = evict_if_needed(&store, 999999999);
    assert!(result);
}

// ==================== Configuration Tests ====================

#[test]
fn test_config_defaults() {
    let config = Config::default();
    
    assert_eq!(config.server.bind, "127.0.0.1");
    assert_eq!(config.server.port, 6379);
    assert_eq!(config.server.num_shards, 256);
    assert_eq!(config.server.batch_size, 16);
    assert_eq!(config.server.max_connections, 10000);
    assert_eq!(config.memory.max_memory, 0);
    assert_eq!(config.memory.eviction_policy, "allkeys-lru");
    assert_eq!(config.security.password, "");
    assert_eq!(config.security.tls_enabled, false);
}

#[test]
fn test_connection_state_default() {
    let state = ConnectionState::new();
    // In the lib.rs version, authenticated defaults to true for testing
    assert!(state.authenticated);
}

// ==================== Concurrent Access Tests ====================

#[test]
fn test_concurrent_access() {
    let store = Arc::new(create_test_store());
    let mut handles = vec![];
    
    // Spawn 10 threads doing concurrent operations
    for i in 0..10 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            for j in 0..100 {
                let key = Bytes::from(format!("key{}_{}", i, j));
                let value = Bytes::from(format!("value{}_{}", i, j));
                
                store_clone.set(key.clone(), value.clone(), None, get_timestamp());
                
                let result = store_clone.get(&key, get_timestamp());
                assert!(result.is_some());
            }
        });
        handles.push(handle);
    }
    
    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
    
    // Should have 1000 keys total
    assert_eq!(store.len(), 1000);
}

// ==================== Delete Tests ====================

#[test]
fn test_delete_nonexistent_keys() {
    let store = create_test_store();
    
    let keys = vec![
        Bytes::from("nonexistent1"),
        Bytes::from("nonexistent2"),
    ];
    
    let count = store.delete(&keys);
    assert_eq!(count, 0);
}

#[test]
fn test_mixed_delete() {
    let store = create_test_store();
    
    // Create 2 keys
    store.set(Bytes::from("exists1"), Bytes::from("v1"), None, now());
    store.set(Bytes::from("exists2"), Bytes::from("v2"), None, now());
    
    // Try to delete 2 existing and 2 nonexistent
    let keys = vec![
        Bytes::from("exists1"),
        Bytes::from("nonexistent1"),
        Bytes::from("exists2"),
        Bytes::from("nonexistent2"),
    ];
    
    let count = store.delete(&keys);
    assert_eq!(count, 2);
}

#[test]
fn test_stress_delete() {
    let store = create_test_store();
    
    // Add 1000 keys
    for i in 0..1000 {
        store.set(
            Bytes::from(format!("key{}", i)),
            Bytes::from("value"),
            None,
            now()
        );
    }
    
    // Delete them all at once
    let keys: Vec<Bytes> = (0..1000)
        .map(|i| Bytes::from(format!("key{}", i)))
        .collect();
    
    let count = store.delete(&keys);
    assert_eq!(count, 1000);
    assert_eq!(store.len(), 0);
}

// ==================== Edge Cases & Special Values ====================

#[test]
fn test_large_values() {
    let store = create_test_store();
    
    // Test with 1MB value
    let key = Bytes::from("large_key");
    let large_value = Bytes::from(vec![b'X'; 1024 * 1024]);
    
    store.set(key.clone(), large_value.clone(), None, now());
    
    let result = store.get(&key, now()).unwrap();
    assert_eq!(result.len(), 1024 * 1024);
    assert_eq!(result, large_value);
}

#[test]
fn test_many_keys() {
    let store = create_test_store();
    
    // Insert 10,000 keys
    for i in 0..10000 {
        store.set(
            Bytes::from(format!("key{}", i)),
            Bytes::from(format!("value{}", i)),
            None,
            now()
        );
    }
    
    assert_eq!(store.len(), 10000);
    
    // Verify random samples
    for i in (0..10000).step_by(1000) {
        let key = Bytes::from(format!("key{}", i));
        let expected = Bytes::from(format!("value{}", i));
        assert_eq!(store.get(&key, now()).unwrap(), expected);
    }
}

#[test]
fn test_empty_key() {
    let store = create_test_store();
    let empty_key = Bytes::from("");
    let value = Bytes::from("value");
    
    store.set(empty_key.clone(), value.clone(), None, now());
    
    let result = store.get(&empty_key, now());
    assert!(result.is_some());
    assert_eq!(result.unwrap(), value);
}

#[test]
fn test_binary_values() {
    let store = create_test_store();
    let key = Bytes::from("binary");
    
    // Value with null bytes and binary data
    let binary_value = Bytes::from(vec![0u8, 1, 2, 255, 254, 0, 0, 128]);
    
    store.set(key.clone(), binary_value.clone(), None, now());
    
    let result = store.get(&key, now()).unwrap();
    assert_eq!(result, binary_value);
}

#[test]
fn test_unicode_keys_and_values() {
    let store = create_test_store();
    
    let key = Bytes::from("키");
    let value = Bytes::from("值");
    
    store.set(key.clone(), value.clone(), None, now());
    
    let result = store.get(&key, now()).unwrap();
    assert_eq!(result, value);
}

// ==================== Sharding Tests ====================

#[test]
fn test_shard_distribution() {
    let store = create_test_store();
    
    // Insert keys and verify they're distributed across shards
    for i in 0..100 {
        store.set(
            Bytes::from(format!("key{}", i)),
            Bytes::from("value"),
            None,
            now()
        );
    }
    
    // Check that multiple shards are used
    let mut shards_used = 0;
    for shard in &store.shards {
        if !shard.is_empty() {
            shards_used += 1;
        }
    }
    
    // With 100 keys and 16 shards, we should use most shards
    assert!(shards_used > 10);
}

// ==================== Entry Tests ====================

#[test]
fn test_entry_clone() {
    use std::sync::atomic::{AtomicU32, Ordering};
    
    let entry1 = Entry {
        value: Bytes::from("test"),
        expiry: Some(12345),
        last_accessed: AtomicU32::new(100),
    };
    
    let entry2 = entry1.clone();
    
    assert_eq!(entry1.value, entry2.value);
    assert_eq!(entry1.expiry, entry2.expiry);
    assert_eq!(
        entry1.last_accessed.load(Ordering::Relaxed),
        entry2.last_accessed.load(Ordering::Relaxed)
    );
}

