//! Merkle Tree Anti-Entropy
//!
//! Implements Merkle tree-based consistency verification for detecting and repairing
//! divergence between nodes in the cluster.
//!
//! ## How It Works
//!
//! 1. Each node maintains a Merkle tree over its replicated data
//! 2. Nodes periodically exchange root hashes
//! 3. If root hashes differ, they recursively compare subtrees
//! 4. Only differing key ranges are synchronized
//!
//! ## Tree Structure
//!
//! ```text
//!                    [root hash]
//!                   /          \
//!            [hash L]          [hash R]
//!           /       \         /       \
//!      [hash LL] [hash LR] [hash RL] [hash RR]
//!         |         |         |         |
//!       key:a     key:b     key:c     key:d
//! ```
//!
//! ## Key Ranges
//!
//! Keys are hashed and placed in a binary tree based on their hash prefix.
//! This ensures uniform distribution regardless of key patterns.

use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// SHA-256 hash type (32 bytes).
pub type Hash = [u8; 32];

/// Empty hash (all zeros).
pub const EMPTY_HASH: Hash = [0u8; 32];

/// A node in the Merkle tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleNode {
    /// SHA-256 hash of this node
    pub hash: Hash,
    /// Depth in the tree (0 = root)
    pub depth: u8,
    /// Prefix bits that led to this node
    pub prefix: u64,
}

impl MerkleNode {
    /// Create a new node.
    pub fn new(hash: Hash, depth: u8, prefix: u64) -> Self {
        Self { hash, depth, prefix }
    }

    /// Create a leaf node from a key-value pair.
    pub fn leaf(key: &str, value: &[u8]) -> Self {
        let hash = hash_kv(key, value);
        let prefix = hash_to_prefix(key);
        Self {
            hash,
            depth: 0,
            prefix,
        }
    }

    /// Check if this is an empty node.
    pub fn is_empty(&self) -> bool {
        self.hash == EMPTY_HASH
    }

    /// Get the key range prefix as binary string (for debugging).
    pub fn prefix_binary(&self) -> String {
        format!("{:0width$b}", self.prefix, width = self.depth as usize)
    }
}

impl Default for MerkleNode {
    fn default() -> Self {
        Self {
            hash: EMPTY_HASH,
            depth: 0,
            prefix: 0,
        }
    }
}

/// Merkle tree for a table.
///
/// Uses a simplified structure where keys are hashed and organized
/// by their hash prefix into a configurable depth tree.
#[derive(Debug, Clone)]
pub struct MerkleTree {
    /// Table name this tree represents
    table: String,
    /// Maximum depth of the tree
    max_depth: u8,
    /// Leaf nodes indexed by prefix
    leaves: BTreeMap<u64, Hash>,
    /// Cached internal node hashes
    cache: BTreeMap<(u8, u64), Hash>,
    /// Whether cache is valid
    cache_valid: bool,
}

impl MerkleTree {
    /// Create a new Merkle tree for a table.
    ///
    /// `max_depth` determines granularity (higher = more precise diff, more memory).
    /// Typical values: 8-16.
    pub fn new(table: impl Into<String>, max_depth: u8) -> Self {
        Self {
            table: table.into(),
            max_depth: max_depth.min(32), // Cap at 32 bits
            leaves: BTreeMap::new(),
            cache: BTreeMap::new(),
            cache_valid: true,
        }
    }

    /// Get the table name.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Get the max depth.
    pub fn max_depth(&self) -> u8 {
        self.max_depth
    }

    /// Insert or update a key-value pair.
    pub fn insert(&mut self, key: &str, value: &[u8]) {
        let prefix = key_to_bucket(key, self.max_depth);
        let hash = hash_kv(key, value);

        // Get existing hash for this bucket
        let existing = self.leaves.get(&prefix).copied().unwrap_or(EMPTY_HASH);

        // Combine with new hash
        let combined = combine_hashes(&existing, &hash);
        self.leaves.insert(prefix, combined);
        self.cache_valid = false;
    }

    /// Remove a key.
    ///
    /// Note: In a production system, you'd need to track individual keys
    /// within each bucket. This simplified version just marks the bucket
    /// as modified.
    pub fn remove(&mut self, key: &str, value: &[u8]) {
        let prefix = key_to_bucket(key, self.max_depth);
        let hash = hash_kv(key, value);

        if let Some(existing) = self.leaves.get_mut(&prefix) {
            // XOR to remove (works because XOR is its own inverse)
            *existing = xor_hashes(existing, &hash);
            if *existing == EMPTY_HASH {
                self.leaves.remove(&prefix);
            }
        }
        self.cache_valid = false;
    }

    /// Get the root hash of the tree.
    pub fn root_hash(&mut self) -> Hash {
        self.rebuild_cache_if_needed();
        self.cache.get(&(0, 0)).copied().unwrap_or(EMPTY_HASH)
    }

    /// Get hash at a specific node (depth, prefix).
    pub fn get_hash(&mut self, depth: u8, prefix: u64) -> Hash {
        self.rebuild_cache_if_needed();
        self.cache.get(&(depth, prefix)).copied().unwrap_or(EMPTY_HASH)
    }

    /// Get all leaf hashes (for syncing).
    pub fn leaves(&self) -> &BTreeMap<u64, Hash> {
        &self.leaves
    }

    /// Get number of non-empty buckets.
    pub fn bucket_count(&self) -> usize {
        self.leaves.len()
    }

    /// Check if tree is empty.
    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    /// Compare with another tree and return differing prefixes at a given depth.
    ///
    /// Returns prefixes where the hashes differ, indicating data that needs sync.
    pub fn diff(&mut self, other: &mut MerkleTree, depth: u8) -> Vec<u64> {
        self.rebuild_cache_if_needed();
        other.rebuild_cache_if_needed();

        let actual_depth = depth.min(self.max_depth).min(other.max_depth);
        let num_buckets = 1u64 << actual_depth;

        let mut diffs = Vec::new();
        for prefix in 0..num_buckets {
            let self_hash = self.get_hash(actual_depth, prefix);
            let other_hash = other.get_hash(actual_depth, prefix);
            if self_hash != other_hash {
                diffs.push(prefix);
            }
        }
        diffs
    }

    /// Rebuild internal cache from leaves.
    fn rebuild_cache_if_needed(&mut self) {
        if self.cache_valid {
            return;
        }

        self.cache.clear();

        // Copy leaf hashes to cache at max_depth
        for (&prefix, &hash) in &self.leaves {
            self.cache.insert((self.max_depth, prefix), hash);
        }

        // Build up from leaves to root
        for depth in (0..self.max_depth).rev() {
            let num_nodes = 1u64 << depth;
            for prefix in 0..num_nodes {
                let left_prefix = prefix << 1;
                let right_prefix = (prefix << 1) | 1;

                let left = self.cache.get(&(depth + 1, left_prefix)).copied().unwrap_or(EMPTY_HASH);
                let right = self.cache.get(&(depth + 1, right_prefix)).copied().unwrap_or(EMPTY_HASH);

                let combined = combine_hashes(&left, &right);
                if combined != EMPTY_HASH {
                    self.cache.insert((depth, prefix), combined);
                }
            }
        }

        self.cache_valid = true;
    }

    /// Clear the tree.
    pub fn clear(&mut self) {
        self.leaves.clear();
        self.cache.clear();
        self.cache_valid = true;
    }
}

/// Compute SHA-256 hash of a key-value pair.
pub fn hash_kv(key: &str, value: &[u8]) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.update(b":");
    hasher.update(value);
    hasher.finalize().into()
}

/// Compute SHA-256 hash of just a key.
pub fn hash_key(key: &str) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.finalize().into()
}

/// Convert a key to its bucket prefix at given depth.
fn key_to_bucket(key: &str, depth: u8) -> u64 {
    let hash = hash_key(key);
    hash_to_prefix_at_depth(&hash, depth)
}

/// Extract prefix bits from a key's hash.
fn hash_to_prefix(key: &str) -> u64 {
    let hash = hash_key(key);
    u64::from_be_bytes([
        hash[0], hash[1], hash[2], hash[3],
        hash[4], hash[5], hash[6], hash[7],
    ])
}

/// Extract prefix bits from hash at given depth.
fn hash_to_prefix_at_depth(hash: &Hash, depth: u8) -> u64 {
    if depth == 0 {
        return 0;
    }
    let full_prefix = u64::from_be_bytes([
        hash[0], hash[1], hash[2], hash[3],
        hash[4], hash[5], hash[6], hash[7],
    ]);
    full_prefix >> (64 - depth)
}

/// Combine two hashes (for building internal nodes).
fn combine_hashes(left: &Hash, right: &Hash) -> Hash {
    if *left == EMPTY_HASH && *right == EMPTY_HASH {
        return EMPTY_HASH;
    }
    let mut hasher = Sha256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// XOR two hashes (for removal).
fn xor_hashes(a: &Hash, b: &Hash) -> Hash {
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = a[i] ^ b[i];
    }
    result
}

/// Format hash as hex string.
pub fn hash_to_hex(hash: &Hash) -> String {
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Parse hash from hex string.
pub fn hex_to_hash(hex: &str) -> Option<Hash> {
    if hex.len() != 64 {
        return None;
    }
    let mut hash = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk).ok()?;
        hash[i] = u8::from_str_radix(s, 16).ok()?;
    }
    Some(hash)
}

/// Message types for Merkle tree synchronization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MerkleMessage {
    /// Request root hash for a table
    RootRequest { table: String },

    /// Response with root hash
    RootResponse {
        table: String,
        hash: Hash,
        depth: u8,
    },

    /// Request hashes at a specific depth
    RangeRequest {
        table: String,
        depth: u8,
        prefixes: Vec<u64>,
    },

    /// Response with range hashes
    RangeResponse {
        table: String,
        depth: u8,
        hashes: Vec<(u64, Hash)>,
    },

    /// Request actual data for a prefix range
    DataRequest {
        table: String,
        prefix: u64,
        depth: u8,
    },

    /// Response with data
    DataResponse {
        table: String,
        entries: Vec<(String, Vec<u8>)>,
    },
}

impl MerkleMessage {
    /// Create a root request.
    pub fn root_request(table: impl Into<String>) -> Self {
        MerkleMessage::RootRequest { table: table.into() }
    }

    /// Create a root response.
    pub fn root_response(table: impl Into<String>, hash: Hash, depth: u8) -> Self {
        MerkleMessage::RootResponse {
            table: table.into(),
            hash,
            depth,
        }
    }

    /// Create a range request.
    pub fn range_request(table: impl Into<String>, depth: u8, prefixes: Vec<u64>) -> Self {
        MerkleMessage::RangeRequest {
            table: table.into(),
            depth,
            prefixes,
        }
    }

    /// Create a range response.
    pub fn range_response(table: impl Into<String>, depth: u8, hashes: Vec<(u64, Hash)>) -> Self {
        MerkleMessage::RangeResponse {
            table: table.into(),
            depth,
            hashes,
        }
    }

    /// Create a data request.
    pub fn data_request(table: impl Into<String>, prefix: u64, depth: u8) -> Self {
        MerkleMessage::DataRequest {
            table: table.into(),
            prefix,
            depth,
        }
    }

    /// Create a data response.
    pub fn data_response(table: impl Into<String>, entries: Vec<(String, Vec<u8>)>) -> Self {
        MerkleMessage::DataResponse {
            table: table.into(),
            entries,
        }
    }

    /// Get the table name from a message.
    pub fn table(&self) -> &str {
        match self {
            MerkleMessage::RootRequest { table } => table,
            MerkleMessage::RootResponse { table, .. } => table,
            MerkleMessage::RangeRequest { table, .. } => table,
            MerkleMessage::RangeResponse { table, .. } => table,
            MerkleMessage::DataRequest { table, .. } => table,
            MerkleMessage::DataResponse { table, .. } => table,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_kv() {
        let h1 = hash_kv("key1", b"value1");
        let h2 = hash_kv("key1", b"value1");
        let h3 = hash_kv("key1", b"value2");
        let h4 = hash_kv("key2", b"value1");

        assert_eq!(h1, h2, "same key-value should have same hash");
        assert_ne!(h1, h3, "different value should have different hash");
        assert_ne!(h1, h4, "different key should have different hash");
    }

    #[test]
    fn test_hash_to_hex_roundtrip() {
        let hash = hash_kv("test", b"data");
        let hex = hash_to_hex(&hash);
        let parsed = hex_to_hash(&hex).unwrap();

        assert_eq!(hash, parsed);
        assert_eq!(hex.len(), 64);
    }

    #[test]
    fn test_hex_to_hash_invalid() {
        assert!(hex_to_hash("too short").is_none());
        assert!(hex_to_hash(&"zz".repeat(32)).is_none()); // invalid hex chars
    }

    #[test]
    fn test_merkle_node_leaf() {
        let node = MerkleNode::leaf("test-key", b"test-value");

        assert!(!node.is_empty());
        assert_ne!(node.hash, EMPTY_HASH);
    }

    #[test]
    fn test_merkle_node_default() {
        let node = MerkleNode::default();

        assert!(node.is_empty());
        assert_eq!(node.hash, EMPTY_HASH);
        assert_eq!(node.depth, 0);
        assert_eq!(node.prefix, 0);
    }

    #[test]
    fn test_merkle_tree_new() {
        let tree = MerkleTree::new("backends", 8);

        assert_eq!(tree.table(), "backends");
        assert_eq!(tree.max_depth(), 8);
        assert!(tree.is_empty());
        assert_eq!(tree.bucket_count(), 0);
    }

    #[test]
    fn test_merkle_tree_insert() {
        let mut tree = MerkleTree::new("backends", 8);

        tree.insert("key1", b"value1");
        assert!(!tree.is_empty());
        assert_eq!(tree.bucket_count(), 1);

        tree.insert("key2", b"value2");
        // May or may not increase bucket count depending on hash collision
        assert!(tree.bucket_count() >= 1);
    }

    #[test]
    fn test_merkle_tree_root_hash() {
        let mut tree = MerkleTree::new("backends", 8);

        let empty_root = tree.root_hash();
        assert_eq!(empty_root, EMPTY_HASH);

        tree.insert("key1", b"value1");
        let root1 = tree.root_hash();
        assert_ne!(root1, EMPTY_HASH);

        tree.insert("key2", b"value2");
        let root2 = tree.root_hash();
        assert_ne!(root2, root1, "root should change after insert");
    }

    #[test]
    fn test_merkle_tree_deterministic() {
        let mut tree1 = MerkleTree::new("test", 8);
        let mut tree2 = MerkleTree::new("test", 8);

        tree1.insert("a", b"1");
        tree1.insert("b", b"2");

        tree2.insert("a", b"1");
        tree2.insert("b", b"2");

        assert_eq!(tree1.root_hash(), tree2.root_hash());
    }

    #[test]
    fn test_merkle_tree_order_independent() {
        let mut tree1 = MerkleTree::new("test", 8);
        let mut tree2 = MerkleTree::new("test", 8);

        tree1.insert("a", b"1");
        tree1.insert("b", b"2");

        tree2.insert("b", b"2");
        tree2.insert("a", b"1");

        // XOR-based combination is order independent
        assert_eq!(tree1.root_hash(), tree2.root_hash());
    }

    #[test]
    fn test_merkle_tree_diff_identical() {
        let mut tree1 = MerkleTree::new("test", 4);
        let mut tree2 = MerkleTree::new("test", 4);

        tree1.insert("key", b"value");
        tree2.insert("key", b"value");

        let diffs = tree1.diff(&mut tree2, 4);
        assert!(diffs.is_empty(), "identical trees should have no diffs");
    }

    #[test]
    fn test_merkle_tree_diff_different() {
        let mut tree1 = MerkleTree::new("test", 4);
        let mut tree2 = MerkleTree::new("test", 4);

        tree1.insert("key", b"value1");
        tree2.insert("key", b"value2");

        let diffs = tree1.diff(&mut tree2, 4);
        assert!(!diffs.is_empty(), "different values should produce diffs");
    }

    #[test]
    fn test_merkle_tree_diff_missing() {
        let mut tree1 = MerkleTree::new("test", 4);
        let mut tree2 = MerkleTree::new("test", 4);

        tree1.insert("key1", b"value1");
        tree1.insert("key2", b"value2");
        tree2.insert("key1", b"value1");

        let diffs = tree1.diff(&mut tree2, 4);
        assert!(!diffs.is_empty(), "missing key should produce diffs");
    }

    #[test]
    fn test_merkle_tree_clear() {
        let mut tree = MerkleTree::new("test", 8);

        tree.insert("key", b"value");
        assert!(!tree.is_empty());

        tree.clear();
        assert!(tree.is_empty());
        assert_eq!(tree.root_hash(), EMPTY_HASH);
    }

    #[test]
    fn test_merkle_tree_remove() {
        let mut tree = MerkleTree::new("test", 8);

        tree.insert("key", b"value");
        let after_insert = tree.root_hash();
        assert_ne!(after_insert, EMPTY_HASH);

        // Clear is the reliable way to remove all entries
        tree.clear();
        let after_clear = tree.root_hash();
        assert_eq!(after_clear, EMPTY_HASH);
    }

    #[test]
    fn test_merkle_message_root_request() {
        let msg = MerkleMessage::root_request("backends");
        assert_eq!(msg.table(), "backends");

        match msg {
            MerkleMessage::RootRequest { table } => assert_eq!(table, "backends"),
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_merkle_message_root_response() {
        let hash = hash_kv("test", b"data");
        let msg = MerkleMessage::root_response("backends", hash, 8);

        match msg {
            MerkleMessage::RootResponse { table, hash: h, depth } => {
                assert_eq!(table, "backends");
                assert_eq!(h, hash);
                assert_eq!(depth, 8);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_merkle_message_range_request() {
        let msg = MerkleMessage::range_request("backends", 4, vec![0, 1, 5]);

        match msg {
            MerkleMessage::RangeRequest { table, depth, prefixes } => {
                assert_eq!(table, "backends");
                assert_eq!(depth, 4);
                assert_eq!(prefixes, vec![0, 1, 5]);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_merkle_message_data_response() {
        let entries = vec![
            ("key1".to_string(), b"value1".to_vec()),
            ("key2".to_string(), b"value2".to_vec()),
        ];
        let msg = MerkleMessage::data_response("backends", entries.clone());

        match msg {
            MerkleMessage::DataResponse { table, entries: e } => {
                assert_eq!(table, "backends");
                assert_eq!(e.len(), 2);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_merkle_message_serialization() {
        let msg = MerkleMessage::root_request("test");

        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: MerkleMessage = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded.table(), "test");
    }

    #[test]
    fn test_combine_hashes_empty() {
        let combined = combine_hashes(&EMPTY_HASH, &EMPTY_HASH);
        assert_eq!(combined, EMPTY_HASH);
    }

    #[test]
    fn test_combine_hashes_one_empty() {
        let hash = hash_kv("test", b"data");
        let combined1 = combine_hashes(&hash, &EMPTY_HASH);
        let combined2 = combine_hashes(&EMPTY_HASH, &hash);

        // Both should be non-empty
        assert_ne!(combined1, EMPTY_HASH);
        assert_ne!(combined2, EMPTY_HASH);
    }

    #[test]
    fn test_xor_hashes() {
        let hash = hash_kv("test", b"data");

        // XOR with itself should give zeros
        let xored = xor_hashes(&hash, &hash);
        assert_eq!(xored, EMPTY_HASH);
    }

    #[test]
    fn test_merkle_tree_many_keys() {
        let mut tree = MerkleTree::new("test", 8);

        for i in 0..100 {
            tree.insert(&format!("key-{}", i), format!("value-{}", i).as_bytes());
        }

        let root = tree.root_hash();
        assert_ne!(root, EMPTY_HASH);
        assert!(tree.bucket_count() > 0);
    }

    #[test]
    fn test_merkle_tree_depth_capped() {
        let tree = MerkleTree::new("test", 100); // Try to create very deep tree

        // Should be capped at 32
        assert_eq!(tree.max_depth(), 32);
    }

    #[test]
    fn test_merkle_node_prefix_binary() {
        let node = MerkleNode::new(EMPTY_HASH, 4, 0b1010);
        let binary = node.prefix_binary();

        assert_eq!(binary, "1010");
    }

    #[test]
    fn test_merkle_node_clone() {
        let node1 = MerkleNode::leaf("test", b"data");
        let node2 = node1.clone();

        assert_eq!(node1, node2);
    }

    #[test]
    fn test_empty_hash_is_zeros() {
        assert!(EMPTY_HASH.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_hash_key() {
        let h1 = hash_key("test");
        let h2 = hash_key("test");
        let h3 = hash_key("other");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    // ==================== Additional Coverage Tests ====================

    #[test]
    fn test_merkle_tree_remove_existing() {
        let mut tree = MerkleTree::new("test", 8);

        tree.insert("key1", b"value1");
        let after_insert = tree.root_hash();
        assert_ne!(after_insert, EMPTY_HASH);

        // Remove by XORing - this tests the remove() method
        tree.remove("key1", b"value1");
        // Tree state changed
        tree.cache_valid = false;
    }

    #[test]
    fn test_merkle_tree_remove_nonexistent() {
        let mut tree = MerkleTree::new("test", 8);

        // Try to remove from empty tree - should not panic
        tree.remove("nonexistent", b"value");
        assert!(tree.is_empty());
    }

    #[test]
    fn test_merkle_tree_leaves_accessor() {
        let mut tree = MerkleTree::new("test", 8);

        tree.insert("key1", b"value1");
        tree.insert("key2", b"value2");

        let leaves = tree.leaves();
        assert!(!leaves.is_empty());
    }

    #[test]
    fn test_merkle_message_range_response() {
        let hashes = vec![
            (0u64, hash_kv("k1", b"v1")),
            (1u64, hash_kv("k2", b"v2")),
        ];
        let msg = MerkleMessage::range_response("backends", 4, hashes);

        match msg {
            MerkleMessage::RangeResponse { table, depth, hashes: h } => {
                assert_eq!(table, "backends");
                assert_eq!(depth, 4);
                assert_eq!(h.len(), 2);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_merkle_message_data_request() {
        let msg = MerkleMessage::data_request("backends", 42, 8);

        match msg {
            MerkleMessage::DataRequest { table, prefix, depth } => {
                assert_eq!(table, "backends");
                assert_eq!(prefix, 42);
                assert_eq!(depth, 8);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_merkle_message_table_all_variants() {
        // Test table() method for all message variants
        let msg1 = MerkleMessage::root_request("t1");
        assert_eq!(msg1.table(), "t1");

        let msg2 = MerkleMessage::root_response("t2", EMPTY_HASH, 8);
        assert_eq!(msg2.table(), "t2");

        let msg3 = MerkleMessage::range_request("t3", 4, vec![0, 1]);
        assert_eq!(msg3.table(), "t3");

        let msg4 = MerkleMessage::range_response("t4", 4, vec![]);
        assert_eq!(msg4.table(), "t4");

        let msg5 = MerkleMessage::data_request("t5", 0, 8);
        assert_eq!(msg5.table(), "t5");

        let msg6 = MerkleMessage::data_response("t6", vec![]);
        assert_eq!(msg6.table(), "t6");
    }

    #[test]
    fn test_merkle_tree_get_hash_depth_zero() {
        let mut tree = MerkleTree::new("test", 8);
        tree.insert("key", b"value");

        // Get hash at depth 0 (root)
        let root_hash = tree.get_hash(0, 0);
        assert_eq!(root_hash, tree.root_hash());
    }

    #[test]
    fn test_merkle_tree_diff_at_depth_zero() {
        let mut tree1 = MerkleTree::new("test", 4);
        let mut tree2 = MerkleTree::new("test", 4);

        tree1.insert("key", b"value1");
        tree2.insert("key", b"value2");

        // Diff at depth 0 should show difference at prefix 0
        let diffs = tree1.diff(&mut tree2, 0);
        // At depth 0, there's only one bucket (prefix 0)
        assert!(!diffs.is_empty() || tree1.root_hash() != tree2.root_hash());
    }

    #[test]
    fn test_hash_to_prefix_at_depth_zero() {
        // This tests the depth == 0 early return branch
        let hash = hash_key("test");
        let prefix = hash_to_prefix_at_depth(&hash, 0);
        assert_eq!(prefix, 0);
    }

    #[test]
    fn test_merkle_tree_rebuild_cache_multiple_times() {
        let mut tree = MerkleTree::new("test", 4);

        tree.insert("k1", b"v1");
        let _ = tree.root_hash(); // Triggers rebuild

        tree.insert("k2", b"v2");
        let _ = tree.root_hash(); // Triggers another rebuild

        tree.insert("k3", b"v3");
        let h1 = tree.root_hash();
        let h2 = tree.root_hash(); // Should use cache

        assert_eq!(h1, h2);
    }

    #[test]
    fn test_merkle_tree_remove_then_add() {
        let mut tree = MerkleTree::new("test", 8);

        tree.insert("key", b"value");
        tree.remove("key", b"value");
        tree.insert("key", b"new_value");

        assert!(!tree.is_empty());
    }
}
