//! Allocation-amplification regression test for the B-20 asset precompile's `announce` decode
//! path on malformed calldata (Cantina #16 follow-up).
//!
//! Lives in its own integration test binary so the `#[global_allocator]` below is isolated to a
//! single binary, mirroring `crates/common/rpc-types-engine/tests/decode_allocation_bound.rs`.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use alloy_primitives::{Address, Bytes};
use alloy_sol_types::SolCall;
use base_common_precompiles::{
    AssetVersion, B20AssetStorage, B20AssetToken, FakePolicyAccounting, IB20Asset,
    NoopPrecompileCallObserver, PolicyVersion,
};
use base_precompile_storage::{HashMapStorageProvider, StorageCtx};

// Running total of bytes allocated on the current thread. Const-initialized so reading it never
// allocates (which would recurse through the allocator).
thread_local! {
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

/// Global allocator that tallies allocation volume per thread, delegating to the system
/// allocator. Only allocation (growth) is counted, which is what a resource-exhaustion bound
/// cares about.
struct CountingAllocator;

// SAFETY: every call is forwarded to the system allocator with an unchanged layout, so all
// `GlobalAlloc` invariants are those of `System`; the wrapper only records the requested size on
// a successful allocation.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is forwarded unchanged from the caller, upholding `System::alloc`'s
        // contract.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let _ = ALLOCATED.try_with(|c| c.set(c.get().saturating_add(layout.size())));
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` come straight from the caller and originate from
        // `System.alloc`, satisfying `System::dealloc`.
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static COUNTING_ALLOCATOR: CountingAllocator = CountingAllocator;

const CHAIN_ID: u64 = 8453;
const TOKEN: Address = Address::repeat_byte(0x22);
const ALICE: Address = Address::repeat_byte(0xA1);

/// Builds `announce` calldata where `aliases` `bytes[]` element offsets all point at one shared
/// `tail`, with `id` corrupted to invalid UTF-8. Standalone re-implementation of the wire-shape
/// helper in `b20_asset::dispatch`'s unit tests — that helper is private to the crate, so an
/// integration test can't reuse it directly.
fn aliased_malformed_announce_calldata(aliases: usize, tail: &[u8]) -> Vec<u8> {
    assert!(aliases >= 1, "need at least one aliased entry");
    let base = IB20Asset::announceCall {
        internalCalls: vec![Bytes::copy_from_slice(tail)],
        id: String::from("aliased-id"),
        description: String::from("desc"),
        uri: String::new(),
    }
    .abi_encode();

    let args = &base[4..];
    let read_off = |at: usize| -> usize {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&args[at + 24..at + 32]);
        u64::from_be_bytes(buf) as usize
    };
    let write_off = |out: &mut [u8], at: usize, v: usize| {
        out[at..at + 32].fill(0);
        out[at + 24..at + 32].copy_from_slice(&(v as u64).to_be_bytes());
    };

    let off_calls = read_off(0);
    let off_id = read_off(32);
    let off_desc = read_off(64);
    let off_uri = read_off(96);
    assert_eq!(read_off(off_calls), 1, "base encoding must be one element");

    // Base array section (n == 1): [len=1][off0][blob..]; the three strings begin at off_id.
    let blob = &args[off_calls + 64..off_id];
    let strings = &args[off_id..];

    let extra = (aliases - 1) * 32; // widening the offset table shifts everything after it
    let shared_elem_off = aliases * 32; // offset (from after the length word) of the shared blob

    let mut out = base[..4].to_vec();
    out.resize(4 + off_calls + 32 + aliases * 32 + blob.len() + strings.len(), 0);
    let a = &mut out[4..];
    write_off(a, 0, off_calls);
    write_off(a, 32, off_id + extra);
    write_off(a, 64, off_desc + extra);
    write_off(a, 96, off_uri + extra);
    write_off(a, off_calls, aliases); // array length
    for i in 0..aliases {
        write_off(a, off_calls + 32 + i * 32, shared_elem_off);
    }
    let blob_at = off_calls + 32 + aliases * 32;
    a[blob_at..blob_at + blob.len()].copy_from_slice(blob);
    let strings_at = off_id + extra;
    a[strings_at..strings_at + strings.len()].copy_from_slice(strings);

    // Corrupt `"aliased-id"` in place to invalid UTF-8, triggering `valid_token` rejection.
    let marker = b"aliased-id";
    let at = out.windows(marker.len()).position(|w| w == marker).expect("marker present");
    out[at..at + marker.len()].fill(0xff);
    out
}

/// Routes `calldata` at `version` and returns the bytes this thread allocated doing it. Decode
/// fails before any storage access, so the provider never needs token initialization.
fn allocated_routing(version: AssetVersion, calldata: &[u8]) -> usize {
    let policy_version = match version {
        AssetVersion::V1 => PolicyVersion::V1,
        AssetVersion::V2 => PolicyVersion::V2,
    };
    let mut storage = HashMapStorageProvider::new(CHAIN_ID);
    storage.set_caller(ALICE);
    ALLOCATED.with(|c| c.set(0));
    let _ = StorageCtx::enter(&mut storage, |ctx| {
        B20AssetToken::with_storage_and_policy(
            B20AssetStorage::from_address(TOKEN, ctx),
            FakePolicyAccounting::new(),
            policy_version,
        )
        .route(ctx, calldata, version, false, NoopPrecompileCallObserver)
    });
    ALLOCATED.with(|c| c.get())
}

/// V1 (Beryl) must keep paying the owned decoder's diagnostic on malformed `announce`, unchanged
/// — its revert bytes are consensus-frozen, so this fix cannot bound it. This asserts the
/// pre-existing, expensive baseline is still exactly that: allocation scales with
/// `aliases * tail_bytes`, not with raw calldata length. A large aliased payload (1,024 aliases,
/// 64 `KiB` tail, matching the reviewer's own worst-case repro) keeps its wire size small
/// (~98 KB) while the owned diagnostic re-encodes every alias's tail, so a lower bound of
/// `aliases * tail.len()` cleanly separates "amplified" from "calldata-bounded."
#[test]
fn announce_malformed_aliased_allocation_v1_scales_with_alias_count() {
    let aliases = 1_024;
    let tail = vec![0u8; 64 * 1024];
    let calldata = aliased_malformed_announce_calldata(aliases, &tail);

    let allocated = allocated_routing(AssetVersion::V1, &calldata);

    assert!(
        allocated > aliases * tail.len(),
        "V1 malformed-announce allocation was {allocated} bytes for {aliases} aliases of a \
         {} byte tail ({} byte calldata); expected it to exceed aliases * tail_bytes, proving \
         the frozen owned-decoder diagnostic is still unbounded on this input",
        tail.len(),
        calldata.len(),
    );
}

/// V2 (Cobalt, not yet scheduled on any network) short-circuits the same malformed payload with a
/// fixed-size rejection before the owned decoder ever runs, so allocation must stay proportional
/// to raw calldata length regardless of alias count (Cantina #16 follow-up).
#[test]
fn announce_malformed_aliased_allocation_v2_is_bounded_by_calldata_length() {
    let aliases = 1_024;
    let tail = vec![0u8; 64 * 1024];
    let calldata = aliased_malformed_announce_calldata(aliases, &tail);

    let allocated = allocated_routing(AssetVersion::V2, &calldata);

    let max_allocation = 8 * calldata.len();
    assert!(
        allocated < max_allocation,
        "V2 malformed-announce allocation was {allocated} bytes for {aliases} aliases of a \
         {} byte tail ({} byte calldata, limit {max_allocation}); the bounded rejection did not \
         short-circuit before the owned decoder ran",
        tail.len(),
        calldata.len(),
    );
}
