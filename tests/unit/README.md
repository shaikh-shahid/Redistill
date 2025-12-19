# Unit Tests

## Current Organization

All unit tests are currently located in `tests/unit_tests.rs` at the repository root's test directory.

This organization keeps unit tests separate from `main.rs` while still allowing them to test internal implementation details through the library interface (`lib.rs`).

## Why Not in This Folder?

In Rust, files in the `tests/` directory are compiled as separate integration test crates. For better organization and to avoid cluttering `main.rs`, we've created `tests/unit_tests.rs` which:

1. Has direct access to the library's public API
2. Tests core functionality without requiring a running server
3. Runs faster than integration tests
4. Provides detailed coverage of individual components

## Future Organization

This folder (`tests/unit/`) is reserved for future expansion when unit tests grow large enough to warrant splitting into multiple modules:

```
tests/unit/
├── store_tests.rs       # Storage layer tests
├── parser_tests.rs      # RESP parser tests  
├── eviction_tests.rs    # Eviction policy tests
└── config_tests.rs      # Configuration tests
```

## Running Unit Tests

```bash
# Run all unit tests
cargo test --test unit_tests

# Run specific test
cargo test --test unit_tests test_sharded_store_set_and_get

# Run with output
cargo test --test unit_tests -- --nocapture
```

## Test Coverage

Current unit tests cover:
- ✅ Basic storage operations (SET, GET, DELETE)
- ✅ TTL and expiration
- ✅ Multiple key operations
- ✅ Concurrent access
- ✅ Eviction policies
- ✅ Configuration defaults
- ✅ Helper functions
- ✅ Edge cases (empty keys, binary values, unicode, large values)
- ✅ Sharding distribution

## Adding New Tests

Add tests to `tests/unit_tests.rs`:

```rust
#[test]
fn test_your_feature() {
    let store = create_test_store();
    // Your test logic here
    assert!(condition);
}
```

Keep tests focused, fast, and independent.

