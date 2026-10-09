# Code-page ownership

`CodeCache` owns code lifetimes and write watches for one guest backing memory
and mapping table. Engine adapters own the actual memories and module handles.

```rust
use wasm86_code_cache::CodeCache;
use wasm86_x86::{CpuState, SegmentProfile};

let mut table = vec![0; 4 << 20];
table[4..8].copy_from_slice(&0x1003u32.to_le_bytes());
let backing = vec![0x90; 65536];
let cpu = CpuState::default();
let mut cache = CodeCache::new(SegmentProfile::Flat32.into(), &table);
let capture = cache.capture(&cpu, 0x1000, 16, &mut table).unwrap();
let code = capture.copy_bytes(&backing); // watches are already active
// Send code and capture.ticket to the compilation worker.
// At an execution boundary, accept a completed module only if still valid:
assert!(cache.install(capture.ticket, &mut table));
assert_eq!(cache.lookup(0x1000), Some(capture.ticket));
```

Each code lifetime gets a unique ticket. Dependencies include CS context, mapping slots
and backing pages. A write through any alias invalidates both pending and
installed tickets. Remapping a slot invalidates its dependents even when it later
returns to the original backing. Failed and superseded requests release their
watches; other entries retain theirs. An installed entry remains usable while a
replacement compiles. There is no code-byte scan or generation comparison at
block entry.

Prepared modules use the same lifetime mechanism without a generation request:

```rust,ignore
use wasm86_code_cache::CodeRange;

let ticket = cache.register(&cpu, 0x1000,
    &[CodeRange { offset: 0x1000, bytes: 3 }], &mut table).unwrap();
// Load or compile an artifact asynchronously while its dependencies are protected.
assert!(cache.install(ticket, &mut table));
```

`CodeRange` describes CS-relative guest bytes. Registration covers every declared
range, including the entry EIP, or fails without adding watches or replacing an
existing registration in that context. Snapshot capture may instead stop at a
mapped prefix, because only that prefix is supplied to generation. Both paths
share range resolution and watch ownership. The loader establishes that prepared
code matches the guest image, runtime ABI and execution profile; registration
reads mapping metadata only. Tickets belong to one cache and remain live until
invalidated or cancelled, independently of worker jobs. `is_pending` distinguishes
a registration awaiting installation from an already installed ticket.

The adapter calls `enter` before selecting an entry, `invalidate_backing` before
host/DMA writes, `invalidate_write` from the generated `invalidateCode` callback,
and `remap` for mapping changes. Remapping updates reverse aliases and protects a
new writable alias before publishing it. Raw backing or table edits after adoption
must obey this same protocol.

Registration, capture and installation happen between Wasm invocations. Capture
must reserve, copy and enqueue synchronously with respect to guest execution.
Device callbacks may invalidate existing tickets. They cannot register or capture
new code while generated access proofs are live. Guest memories are private to
the execution thread; workers receive copied bytes only.

Snapshot copies stop at unavailable backing or the CS limit. The compiler may
accept an earlier block boundary; an incomplete instruction fails compilation and
continues through ordinary interpretation. Watches can conservatively include
trailing snapshot bytes that generation did not consume.
