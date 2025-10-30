use alloy_primitives::B256;
use arc_swap::ArcSwap;
use eyre::Result as EyreResult;
use reth_provider::StateProvider;
use std::sync::Arc;

use crate::FlashblocksState;

/// Trie nodes and hashed state from computing a flashblock state root.
///
/// These cached nodes can be reused when computing a bundle's state root
/// to avoid recalculating the flashblock portion of the trie.
#[derive(Debug, Clone)]
pub struct FlashblockTrieData {
    pub trie_updates: reth_trie_common::updates::TrieUpdates,
    pub hashed_state: reth_trie_common::HashedPostState,
}

/// Internal cache entry for a single flashblock.
#[derive(Debug, Clone)]
struct CachedFlashblockTrie {
    block_hash: B256,
    flashblock_index: u64,
    /// The cached trie data
    trie_data: FlashblockTrieData,
}

/// Thread-safe single-entry cache for the latest flashblock's trie nodes.
///
/// This cache stores the intermediate trie nodes computed when calculating
/// the latest flashblock's state root. Subsequent bundle metering operations
/// on the same flashblock can reuse these cached nodes instead of recalculating
/// them, significantly improving performance.
///
/// **Important**: This cache holds only ONE flashblock's trie at a time.
/// When a new flashblock is cached, it replaces any previously cached flashblock.
/// This design assumes that bundle metering operations are performed on the
/// current/latest flashblock, not historical ones.
#[derive(Debug, Clone)]
pub struct FlashblockTrieCache {
    /// Single-entry cache for the latest flashblock's trie
    cache: Arc<ArcSwap<Option<CachedFlashblockTrie>>>,
}

impl FlashblockTrieCache {
    /// Creates a new empty flashblock trie cache.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(ArcSwap::from_pointee(None)),
        }
    }

    /// Ensures the trie for the given flashblock is cached and returns it.
    ///
    /// If the cache already contains an entry for the given block_hash and flashblock_index,
    /// this returns the cached data immediately without recomputation. Otherwise, it computes
    /// the flashblock's state root, caches the resulting trie nodes, **replacing any previously
    /// cached flashblock**, and returns the new data.
    ///
    /// # Single-Entry Cache Behavior
    ///
    /// This cache only stores one flashblock's trie at a time. Calling this method with a different
    /// flashblock will evict the previous entry. This is by design, as the cache is intended for
    /// the latest/current flashblock only.
    ///
    /// # Arguments
    ///
    /// * `block_hash` - Hash of the block containing the flashblock
    /// * `flashblock_index` - Index of the flashblock within the block
    /// * `flashblocks_state` - The accumulated state from pending flashblocks
    /// * `canonical_state_provider` - State provider for the canonical chain
    ///
    /// # Returns
    ///
    /// The cached `FlashblockTrieData` containing the trie updates and hashed state.
    pub fn ensure_cached(
        &self,
        block_hash: B256,
        flashblock_index: u64,
        flashblocks_state: &FlashblocksState,
        canonical_state_provider: &dyn StateProvider,
    ) -> EyreResult<FlashblockTrieData> {
        // Check if we already have a cached trie for this exact flashblock
        let cached = self.cache.load();
        if let Some(ref cache) = **cached {
            if cache.block_hash == block_hash && cache.flashblock_index == flashblock_index {
                // Cache is still valid for this flashblock, return it
                return Ok(cache.trie_data.clone());
            }
        }

        // Need to compute the flashblock trie (this will replace any existing cache entry)

        // Compute hashed post state from the bundle
        let hashed_state = canonical_state_provider.hashed_post_state(&flashblocks_state.bundle_state);

        // Calculate state root with updates to get the trie nodes
        let (_state_root, trie_updates) = canonical_state_provider.state_root_with_updates(hashed_state.clone())?;

        // Create the trie data
        let trie_data = FlashblockTrieData {
            trie_updates,
            hashed_state,
        };

        // Store the new entry, replacing any previous flashblock's cached trie
        self.cache.store(Arc::new(Some(CachedFlashblockTrie {
            block_hash,
            flashblock_index,
            trie_data: trie_data.clone(),
        })));

        Ok(trie_data)
    }
}

impl Default for FlashblockTrieCache {
    fn default() -> Self {
        Self::new()
    }
}
