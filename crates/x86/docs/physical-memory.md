# Physical memory

`MemoryBackend::PhysicalBus` routes instruction fetch and data accesses on a PC
with guest paging disabled. Segment checks and base addition produce the linear
address. The bus applies A20 and looks up the physical page for both real-mode
and protected-mode segment state. RAM and ROM reads use `guest` backing.
MMIO reads and writes call the host.

## Physical page map

The host supplies shared `guest` backing and an unshared `physicalMap`. Guest
backing has a minimum of one 64-KiB Wasm page and a maximum of 65536. Allocate
enough backing for the configured RAM and ROM.

The physical map has one eight-byte record for each of the 1,048,576 physical
pages in the 32-bit address space. Its entries occupy 8 MiB. A four-byte A20
mask follows, bringing the Wasm import minimum to 129 pages. Unmapped pages
and device mappings have routing records and consume no guest RAM backing.

| Mapping | Reads | Writes |
| --- | --- | --- |
| `Ram { backing_offset }` | Backing bytes | Backing bytes |
| `Rom { backing_offset }` | Backing bytes | Ignored |
| `Mmio` | `readMmio` | `writeMmio` |
| `Unmapped` | FF bytes | Ignored |

Unspecified pages are unmapped. Ignoring ROM/hole writes and returning FF from holes
are this backend's machine policy, not universal x86 behavior. Devices such as
flash that interpret writes must use MMIO. Subpage mixtures of RAM, ROM and MMIO
are not represented. Several physical pages may alias the same backing.

`PhysicalMemoryMap::new(regions)` constructs the table from inclusive ranges:

```rust
use relocade_x86::{PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};

let map = PhysicalMemoryMap::new([
    (0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 }),
    (0xf0000..=0xfffff, PhysicalMapping::Rom { backing_offset: 0xa0000 }),
])?;
# Ok::<(), PhysicalMapError>(())
```

Ranges contain complete pages within the table; later regions replace earlier
ones where they overlap. RAM/ROM backing offsets are page aligned and may address
any part of the host's 32-bit backing memory. `get(address)` resolves one physical
byte for host inspection, adjusting direct offsets to that byte.

`as_bytes()` exposes the routing image for installation at offset zero in
`physicalMap`. Page `p` has two little-endian words. At `p * 8` the kind is
0 for unmapped, 1 for RAM, 2 for ROM, or 3 for MMIO. Bit 2 marks tracked RAM
or ROM. At `p * 8 + 4` the payload is a backing-page offset for RAM or ROM, a
handler index for MMIO, or zero for an unmapped page.

The A20 mask at `PhysicalMemoryMap::A20_MASK_OFFSET` (`0x800000`) is
`0xFFEFFFFF` at startup. It clears address bit 20 before choosing a page or
calling MMIO. Call `set_a20_enabled(true)` and install the updated mask
(`0xFFFFFFFF`) to preserve all 32 address bits. Page entries stay in place.
`get(address)` applies the same gate for host inspection. The host models the
keyboard-controller or fast-A20 port that controls this platform state.

Runtime mapping and gate changes update the installed Wasm table.
Device attachment obtains physical page addresses and serialized handler-word
offsets from `mmio_handler_offsets()`. This includes mappings hidden by A20 and
keeps device publication independent of the routing table's indexing scheme.

## MMIO callbacks

Both functions belong to the `relocade` import module:

| Import | Wasm signature |
| --- | --- |
| `readMmio` | `(address: i32, bytes: i32) -> i64` |
| `writeMmio` | `(address: i32, bytes: i32, value: i64) -> ()` |

Addresses use an unsigned i32 carrier within the real-mode physical range.
`bytes` is a count from 1 through 8; the whole request lies in MMIO.
Values use little-endian order in the low `bytes * 8` bits. Upper read-result bits
are ignored; upper write-value bits are zero. The i64 is a value container, not an
eight-byte device access.

Scalar operand fields of up to eight bytes retain their width when their whole
span is MMIO, including across adjacent MMIO pages. Vector operands involving
MMIO transfer their first eight bytes and then their remaining eight bytes. The
ABI provides no atomicity guarantee across these parts. Wider structured operands
transfer their constituent fields separately.

Each scalar field or vector part is split at a page boundary when its span
crosses different routing kinds. A20 wraparound also splits a request, so each
callback describes contiguous bus addresses. A four-byte field can therefore
leave a three-byte MMIO portion. The host adapter applies its bus and device
rules to each request; one callback need not correspond to one hardware bus
transaction.

Callbacks complete synchronously and execute even when a read result is unused.
They may change backing bytes and installed routing. The next transfer, including
the remaining portion of a split access, reads the current routing. An already
issued MMIO request covers its entire selected span. Callbacks must not inspect or
modify CPU backing state or reenter guest execution. There is no retry result;
unexpected host or Wasm traps are implementation errors.

Execution assumes one guest CPU and one writer to guest backing. Host writes run
synchronously in callbacks or between guest entries. Other threads may observe
memory, but reads can see partial updates. The host must not advance other CPUs or
writing bus masters during an entry. Locked updates use a read followed by a
write under this contract; the ABI does not expose a lock boundary for concurrent
DMA or another processor.

## Generated access and fetch

After the segment check and A20 translation, every 32-bit address has a
physical page entry. The JIT guards complete operand spans: reads must fit
one RAM/ROM page and writes must fit one RAM page. Successful guards retain the
backing address for direct loads and stores. A failed guard publishes completed
instructions and restarts the current instruction in the interpreter, before
consuming device input or changing its destination.

Repeated strings and multi-transfer stack operations check their full spans
before their first transfer. ENTER checks its pushes and display separately;
interrupt entry checks its stack frame and IVT entry. Their loads and stores
remain in architectural order, including when the spans alias. String port I/O
currently uses the interpreter because device callbacks can change mappings
between transfers. The interpreter retains per-element REP progress,
architectural fault ordering and live routing.

An inline check handles interpreter accesses within one RAM/ROM page. Routing,
splitting and partial transfer assembly share one reader and one writer per
module, created only when used. Helpers are Wasm functions; host calls occur only
in their MMIO branches. Separate snapshot modules each contain their own helpers.

The interpreter checks CS and probes RAM/ROM for a direct fetch window without
calling a device. Windows stop at page boundaries; unavailable windows use exact
reads of required bytes. Actual MMIO instruction fetches use one-byte requests
in decoder order, without reading ahead. This is the emulator's functional fetch
contract, not a model of a particular CPU's prefetch bus transactions. Physical
routing is rechecked after each instruction and is not held in the protected
interpreter's page cache.

Snapshot blocks use compiled bytes under the ordinary host validity contract.
Code, mapping and A20 changes must preserve that contract throughout execution; the
interpreter can observe code remapping at the next instruction.
