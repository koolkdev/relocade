# Host integration

Generated x86 modules use this contract for both snapshot blocks and interpreter
entries. The embedding host supplies memory, dispatch and descriptor resolution.
The Wasm engine must support multiple memories, multiple results, tail calls and
the threads extension's atomic instructions, including on unshared memories.

## Entries and dispatch

`CompiledModule` contains the Wasm bytes, exported entry name and required execution
target. Snapshot entries are named `block_<hex start_eip>`; interpreter entries
are named `run` or `step`; the callable dispatcher exports `dispatch`. All have
the Wasm signature `() -> i64`.

`Compiler::new(profile)` uses one execution profile and the default `CpuModel`
for `compile_block`, `compile_interpreter` and `compile_interpreter_step`.
`with_cpu_model(model)` selects the same capabilities for every entry; `target()`
returns the resulting `ExecutionTarget { profile, cpu_model }`. Free compilation
functions use the default model without CPU observations.

Every `CpuFeature` is implemented and enabled by default. For example,
`CpuModel::default().with_feature(CpuFeature::Cmov, false)` disables integer and
x87 conditional moves and the x87 comparisons that set EFLAGS. Known encodings
requiring a disabled capability raise #UD after complete instruction fetch and
before operand access, without retiring. They terminate snapshot compilation before
the next instruction. Snapshot blocks specialize these checks when Wasm is generated.
Interpreters read the configured model from `ExecutionContext`, so their Wasm bytes are
independent of feature choices. Module metadata still records the intended target;
the host must initialize backing and link specialized blocks for that target.

`CpuModel::supports(feature)` reports whether the model enables that capability.
`with_feature` returns the updated model directly. CPUID reports the same enabled
capabilities.
Execution-mode and control-register assumptions remain separate from capability
selection. The model is host configuration, not mutable `CpuState`; keep it fixed
for a guest.

On success, an entry publishes CPU state, updates EIP and the completed-instruction
count, checks its instruction target, then tail-calls
`relocade.dispatch() -> i64`. Dispatch reads the published EIP from CPU state,
and its result is returned unchanged. EIP is an offset relative to CS and wraps
at 32 bits; the instruction count wraps at 64 bits. A taken transfer checks its
segment target before committing instruction effects; fetching the destination
belongs to the next entry.

The host chooses whether dispatch enters more code or returns control. Interpreter
`run` continues inside Wasm until a branch, segment load, port I/O or serializing
instruction completes.
It uses the same instruction boundary as snapshot compilation: conditional branches end
execution on both outcomes. Earlier instructions remain published if a later
instruction faults or is unsupported. Each instruction fetches live guest bytes
and starts with fresh prefix state. REP completes its repetition; port strings
dispatch and other strings continue to the next instruction in both frontends.

Interpreter `step` executes one instruction before its budget check and dispatch,
including all elements of a supported REP instruction.

## Instruction budgets

All execution entries share the declared `ExecutionContext` layout in
`relocade.executionContext`:

```rust
# use relocade_x86::{CpuState, CpuModel};
#[repr(C)]
pub struct ExecutionContext {
    pub cpu: CpuState,
    pub instruction_target: u64,
    pub cpu_model: CpuModel,
}
```

Initialize backing with `ExecutionContext::default().to_bytes()`. Fields are
little endian, with offsets derived from this layout using `offset_of!`.
CPU snapshot reads and writes cover only the `cpu` member. The host sets
`instruction_target` between entries; generated code never writes it.
Its default, `u64::MAX`, is practically unbounded.
Set `cpu_model` to the guest's configured capabilities before execution and keep it
fixed for that guest. Interpreters read it for instruction admission and CPUID;
JIT blocks specialize to it. CPU snapshot replacement preserves this field.
The model is a little-endian 32-bit capability mask at byte offset 496; use
`ExecutionContext::to_bytes()` to encode it. The complete context is 504 bytes,
including four zero padding bytes. Existing backing must be reinitialized when
updating generated modules to this layout.

For a finite budget, add the requested number of instructions to
`instruction_count`, saturating at `u64::MAX`. Handle a zero budget before entering
Wasm. Keep the target fixed during execution. Checks compare the count and target
as unsigned values and assume the instruction counter does not wrap.

A successful block checks the target using the count it just published. Reaching
or exceeding it returns an encoded `ExecutionExit::BudgetExhausted` before
dispatch or fetching the successor.
Interpreter `run` checks only after a block-ending instruction, on either outcome
of a conditional branch. Ordinary instruction retirement, decoder restarts and
optimization handoffs do not introduce budget checkpoints. Faults and unsupported
instructions retain their own exit reasons and precise retirement counts.

This is an instruction allowance, not an exact stopping point. A block can exceed
the target; interpreter straight-line execution and REP work have no fixed bound.
The snapshot compiler's instruction limit bounds compilation only. There is no
fixed overshoot or elapsed-time guarantee.

## Callable dispatch

`Compiler::compile_dispatcher()` generates a module that resolves execution addresses to
ordinary `() -> i64` functions in the imported `relocade.executionFunctions` table. Slot 0
is reserved, slot 1 holds a compatible interpreter, and subsequent slots hold installed
module exports. All functions share CPU, guest and mapping memories. The dispatcher has
no target-kind tag or host-service import.

Bind the module's `dispatch() -> i64` and `interpret() -> i64` exports as the
corresponding imports of execution modules. Both use the published CPU state.
The host starts or resumes execution by calling `dispatch`; generated entries
tail-call the same function after publishing their successor EIP. `interpret`
directly enters the fallback when a snapshot fails an entry guard or optimization
assumption. These transfers stay inside Wasm, and callee results pass through unchanged.

The embedding scopes lookup to one guest address space and execution target (profile and
CPU model). Keys are CS.base + EIP, wrapping to 32 bits, before paging, A20 or backing
translation. The embedding must keep the execution profile valid while calling these
entries. A miss tail-calls the interpreter. Each destination owns its entry checks:
snapshot blocks guard their compiled CS span, and the interpreter checks instruction
fetches. Lookup itself does not validate CS permissions or limits. Registering an export
does not establish its snapshot validity: the embedding must uphold its original EIP,
profile, code-byte and mapping requirements. Every registered entry must be an execution
boundary; larger blocks must not run across another entry. Registration changes
therefore require updating any affected compiled blocks or links. Use the step
interpreter when every interpreted instruction boundary must consult this table; these
transfers remain inside Wasm.

Copy `EntryTable::to_bytes()` at offset zero in `executionEntries`, growing memory
first. The serialized data is a u32 mask followed by eight-byte address/slot rows, with
zero slots marking empty rows. The table uses open addressing and is at most half full.
Replace mappings only outside execution. The function table must already contain every
published slot. Removing an address only changes lookup; the embedding owns
function-slot and code lifetimes.

## Interpreter handoff

A snapshot block can guard execution requirements or optimization assumptions.
If a guard fails, it publishes all completed instructions, leaves EIP at the current
instruction without applying its effects or retiring it, and tail-calls
`relocade.interpret() -> i64`.
Its result is returned unchanged. The captured suffix is abandoned; no compiled
continuation or second copy of its instructions is required.

The host must enter a compatible interpreter sharing the same CPU, guest and
machine memories. It must attempt the current instruction before ordinary dispatch,
so it cannot immediately select the same failing JIT entry. Guest faults and
unsupported instructions retain their usual exits. A failed guard is neither of
those exits.

An embedding can bind this import directly to an instance's `step` or `run` export.
`step` resumes normal dispatch after one instruction; `run` continues to its own
branch, segment-load, port-I/O or serialization boundary. Neither is constrained
by the abandoned snapshot's instruction limit. The host chooses this policy and admits interpreter entries
against their execution target. Interpreter execution uses the shared instruction
semantics without speculative guards and does not import `interpret` itself.

## Observed CPU state

`Compiler::new(profile).specialize_on_cpu(&cpu)` copies a CPU snapshot as
optional input for guarded specialization. Instruction semantics choose which
observed fields to use; currently these are x87 precision and rounding controls,
plus the precision exception mask and flag when both are set, and MXCSR control
settings for SSE numerical operations.
Its `compile_block` method accepts the same EIP, byte snapshot and instruction limit
as the free snapshot functions. Those functions compile with dynamic modes.
Entries generated by `compile_interpreter` and `compile_interpreter_step` read live
CPU state and do not use observations. Later changes to the supplied `CpuState` do
not change the captured snapshot.

The generated block checks a mode only where an instruction consumes it:
x87 arithmetic checks PC and RC, while rounded stores check RC alone. A block without
these consumers has no additional guard. Successful guards let the compiler fold
the unused mode choices and remove repeated checks of unchanged values. FLDCW and
FNINIT establish new control values; subsequent consumers check or fold those
values normally.

When the snapshot has precision exceptions masked and PE already set, arithmetic
also guards those two bits. The compiler can then remove repeated precision
exception calculations, while numerical rounding and C1 updates retain their
usual behavior.
Other observations keep exception masks and flags dynamic; rounded stores still
guard RC alone. FNCLEX and FNINIT clear PE, so a later arithmetic consumer may
hand off to the interpreter even when PC and RC still match. Pending exceptions
take priority over any specialization mismatch, including after FLDCW unmasks PE.

SSE numerical consumers guard MXCSR's rounding, DAZ, FTZ and exception-mask bits.
Sticky status flags and reserved backing bits are excluded from the guard and
remain live, so recording an inexact result does not invalidate the control facts.
LDMXCSR and FXRSTOR replace the tracked value; subsequent consumers check the new
controls. Exact dword-to-binary64 conversions, reciprocal estimates and state
transfers do not add an MXCSR guard. SIMD control guards precede source-memory
reads, with MMX bridge instructions retaining their pending-x87-exception check
first. These guards work in both protected and real modes.

The observation creates no new host admission requirement. A mismatch uses the
ordinary interpreter handoff at the consuming instruction. A host can reuse a
block under another mode; the guard preserves correctness, though frequently
mismatching modes may warrant compiling another block. No CPU state is replaced
with the observed snapshot.

## Processor capabilities

`CpuFeature` contains the implemented CPUID-enumerated instruction capabilities
of the Pentium 4 target. Each is enabled by default and can be disabled independently.

| `CpuFeature` | Instruction capability | CPUID leaf 1 bit |
| --- | --- | --- |
| `Fpu` | x87 floating point | EDX 0 |
| `Tsc` | RDTSC | EDX 4 |
| `Cx8` | CMPXCHG8B | EDX 8 |
| `Cmov` | CMOVcc; FCMOV/FCOMI when FPU is also enabled | EDX 15 |
| `Clflush` | CLFLUSH | EDX 19 |
| `Mmx` | MMX and EMMS | EDX 23 |
| `Fxsr` | FXSAVE/FXRSTOR | EDX 24 |
| `Sse` | SSE, including its MMX additions | EDX 25 |
| `Sse2` | SSE2 | EDX 26 |

SSE3, SYSENTER/SYSEXIT and MONITOR/MWAIT are not implemented or selectable.

Capabilities select instruction extensions independently and do not emulate older
processor generations. They preserve the stored x87, MMX and XMM state. MMX-register
instructions introduced by SSE or SSE2 require that extension, without requiring
the original MMX group: for example, PAVGB requires SSE and PADDQ requires SSE2.
MOVNTI, LFENCE and MFENCE require SSE2; SFENCE and the prefetch hints require SSE.
CLFLUSH has its own feature. FXSAVE/FXRSTOR require FXSR and still transfer the full
retained x87/XMM/MXCSR state when other groups are disabled. FCMOV and FCOMI require
both FPU and CMOV; disabling CMOV preserves the base x87 instruction set.

XMM is the SIMD register bank, not a separate feature. The capability definitions
follow Intel's [P4-era CPUID reference](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366621.pdf#page=213).

## Processor identification

CPUID uses the compiler's selected `CpuModel` in JIT blocks and reads
`ExecutionContext.cpu_model` in interpreters. Neither path needs a host callback.
Leaf 0 reports maximum basic leaf 1 and the vendor string `Relocade CPU`.
Leaf 1 reports virtual family 6, model 0, stepping 1 (EAX=`0x00000601`) and the
enabled capabilities listed above. The default EDX is `0x07888111` and ECX is zero.
Disabling a capability clears its bit and rejects its instructions through the
shared admission mechanism. Unimplemented extensions and platform facilities
remain clear.

With CLFLUSH enabled, EBX bits 15:8 report a 64-byte virtual flush line in eight-byte
units (EBX=`0x00000800`). Disabling CLFLUSH clears both this field and EDX bit 19.
Other EBX fields are zero. The line size defines flush granularity; it does not
model a physical cache. Feature reporting is independent of real/protected mode
and segment assumptions. The identity does not claim a physical Intel processor.

The full EAX selects the leaf. Both leaves ignore input ECX. Unsupported basic and
extended queries return leaf 1, following Intel's highest-basic-leaf rule. There
are no cache, topology, frequency or extended leaves. Results replace the full
EAX, EBX, ECX and EDX in every execution profile; size and segment prefixes do not
narrow the registers. Other registers and all flags are preserved. Clearing
EFLAGS.ID does not disable CPUID. LOCK uses the shared unsupported-encoding exit.

CPUID serializes execution: both a snapshot block and an interpreter run complete
the instruction, publish state and dispatch before fetching its successor. Prior
guest stores are complete at this boundary. The host must validate the next
snapshot's code against any preceding code writes, as at every dispatch; CPUID
does not itself invalidate a host code cache. The single-threaded, unshared-memory
execution contract does not model a processor's speculative pipeline or cache.

These semantics follow Intel's [Volume 2A, CPUID](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366621.pdf#page=202).

## Timestamp counter

RDTSC reads the virtual CPU's 64-bit completed-instruction count into EDX:EAX.
The clock advances by one tick per retired guest instruction. A read includes
preceding instructions in the current block, even before their count is published,
and excludes RDTSC's own retirement. Starting from zero, two consecutive RDTSC
instructions therefore return 0 and 1. Other registers and all flags are preserved.
No host callback or browser timer is needed.

This initial clock models execution progress. It has no fixed relationship to
seconds or physical CPU cycles and stops when guest execution stops. A completed
REP instruction advances it once, regardless of its element count; an instruction
that faults or exits unsupported does not advance it. The default model enables
`CpuFeature::Tsc`, reporting CPUID leaf 1 EDX bit 4, but no clock-frequency or
invariant-TSC leaf. Disabling TSC clears that bit and makes RDTSC raise #UD.
Retirement accounting continues with TSC disabled.

The counter lives in `CpuState::instruction_count` and wraps modulo 2^64. Saving
and restoring the full CPU record preserves it across interpreter entries, JIT
blocks and host dispatches. Hosts may initialize or restore it only between
entries, using all eight bytes. JavaScript hosts should use little-endian
`DataView.getBigUint64` / `setBigUint64` when accessing it directly.

With TSC enabled, RDTSC is not serializing and does not end a block or interpreter
run. Its result reflects guest instruction order. A later fault retains its register effects and
retirement; an earlier fault or incomplete encoding prevents its effects.
All execution profiles support the enabled capability. Protected profiles assume
CR4.TSD=0; changing CR4 or denying timestamp reads with TSD remains unsupported. Real mode
permits the instruction independently of TSD. Operand/address/segment prefixes
do not narrow its full-width registers. LOCK uses the shared unsupported exit.

The register and permission semantics follow Intel's [Volume 2B, RDTSC](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366721.pdf#page=248).
The instruction-based progression is this virtual CPU's explicit timing policy.

## Port I/O

Real16 IN/OUT and INS/OUTS use synchronous `relocade` imports:

| Import | Wasm signature |
| --- | --- |
| `readPort` | `(port: i32, bytes: i32) -> i32` |
| `writePort` | `(port: i32, bytes: i32, value: i32) -> ()` |

The unsigned port address is 0..65535; `bytes` is 1, 2 or 4. An immediate port
is zero-extended from its encoded byte; DX contributes only its low 16 bits.
One callback represents one operand-width transfer, including a word/dword at
port FFFF. The host applies its bus/device rules. Reads ignore upper result bits;
writes zero upper value bits. Reads remain observable even if their result is unused.

Callbacks may change guest backing or physical routing, but must not inspect or
modify CPU backing or reenter execution. They complete synchronously, with no
retry result. Both snapshot blocks and interpreter runs publish completed state
and dispatch after a port instruction, so the host can validate the next code
entry against device changes. Devices and any pending interrupts belong to the host.

Protected profiles fix CPL3/IOPL0 and expose no TSS I/O permission bitmap grants;
port instructions raise #GP(0) after decoding and before device effects. LOCK
uses the shared unsupported-encoding exit. Operand-size overrides choose
word/dword transfers independently of the port address. Segment and address-size
overrides do not affect scalar I/O.

INS stores at ES:(E)DI, ignoring segment overrides. OUTS reads DS:(E)SI with the
usual source override. REP uses the address-sized CX/ECX count; zero count performs
no transfer. Each element keeps its byte/word/dword port width and advances its
index by that width according to DF. The destination is checked before an INS
port read; OUTS completes its memory read before writing the port. Physical routing
is resolved at each actual transfer, including after a port callback.

A fault leaves the completed elements in memory/devices, the remaining count and
current index in CPU backing, and EIP at the string instruction. Successful REP
retires once and dispatches after its final element. There is no device batching
or asynchronous interrupt polling within the repetition.

## Imported memories

All imports belong to the `relocade` module. Memories are distinct, unshared Wasm
memory objects; sizes below are minimum counts of 64-KiB Wasm pages.

| Import | Minimum pages | Contents |
| --- | ---: | --- |
| `executionContext` | 1 | `ExecutionContext` backing, containing CPU state and execution controls. |
| `guest` | 1 | Guest backing bytes for the selected memory model. |
| `machine` | 64 | Protected-mode linear-to-backing page table at byte zero. |
| `physicalMap` | 1 | Real16 physical routing table at byte zero. |
| `executionEntries` | 1 | Execution-address table. |

Protected-mode interpreters import `executionContext`, `guest`, `machine`, `dispatch`,
`resolveSegment` and `querySegmentDescriptor`. Real16 interpreters import
`executionContext`, `guest`, `physicalMap`, `dispatch`, the MMIO callbacks and the port
callbacks. They use generated RAM/ROM accesses and do not call descriptor callbacks. The physical
memory contract below describes routing and callback widths.
Snapshot modules omit imports they do not use: protected-mode memory instructions
require guest backing and the page table, protected-mode segment loads require
`resolveSegment`, and LAR/LSL/VERR/VERW require `querySegmentDescriptor`. Blocks
with specialization guards require `interpret`. Hosts should use the generated
module's import list when instantiating it.
Callable dispatch imports `executionEntries` and the `executionFunctions`
function table. The interpreter itself requires neither import.

The page table contains 2^20 little-endian u32 entries, one per 4-KiB linear page.
Bit 0 means present, bit 1 permits data writes, and bits 12–31 give the physical
frame address in `guest`; other bits are ignored. Reads and instruction fetches
require presence. Every present frame must have valid physical backing. Violating
that invariant is a host error, not a guest page fault.

Use [`page_table::entry_offset`] and [`page_table::encode_entry`] to prepare
individual entry writes. Writing [`page_table::UNMAPPED_ENTRY`] removes a mapping.
The host owns the table storage and applies updates between guest execution calls.

Segment translation precedes this protected-mode backing-map lookup.
Linear addresses wrap at 32 bits, and a span
crossing linear zero uses the mappings on both sides. All pages of a data store
are permission-checked before any byte is written, including scattered backing.
Read-modify-write destinations require write permission even when their value
will remain unchanged. Faults identify the first denied byte.

LOCK is supported on eligible memory-destination forms, including CMPXCHG8B.
In protected mode, naturally aligned operands use native Wasm atomic read-modify-write operations;
memory XCHG does so with or without LOCK. NEG uses a compare-exchange retry loop.
Alignment is determined after segment translation. All operand bytes pass write
checks before the update, including a failed comparison. Ordinary loads, stores
and unlocked updates retain ordinary Wasm accesses; no extra fences are inserted.

Unaligned protected-mode locked operands use checked reads and writes under the private-memory
contract, which prevents guest interleaving. The current imports do not support
concurrent access to shared guest RAM. Such access will require coordination for
unaligned locked operands as well as the surrounding ordinary accesses.
Rejected LOCK combinations and register forms of memory-only instructions remain
unsupported encodings, reported through the unsupported-instruction path rather
than #UD.

## Memory ordering and cache control

Explicit LFENCE, SFENCE and MFENCE instructions use Wasm's full atomic fence.
This supplies stronger ordering than the individual load/store fences require;
it does not change the private-memory contract or model speculative execution.

MOVNTPS, MOVNTPD, MOVNTDQ, MOVNTQ and MOVNTI use the ordinary checked memory
write path. Their non-temporal cache hints do not alter host routing or introduce
buffered writes. The XMM stores require 16-byte linear alignment; MOVNTQ and
MOVNTI do not add an alignment requirement. MMX stores retain MMX exception
admission and the transition of x87 TOP and tags.

MASKMOVQ and MASKMOVDQU check the complete 8- or 16-byte destination span even
when every mask sign bit is clear, a fault policy permitted by Intel. A successful
store writes only bytes whose mask sign bits are set; it never reads destination
data or rewrites untouched bytes. Real16 MMIO therefore sees individual selected
byte writes in increasing address order. MASKMOVQ enters MMX state even when the
mask selects no bytes, provided the destination checks succeed.

PREFETCHNTA/T0/T1/T2 decode their complete memory-address encodings but otherwise
act as ignored hints, without data-access faults or MMIO callbacks. CLFLUSH checks
the addressed byte with read permissions, including execute-only code segments,
and applies a full memory fence. It performs no data transfer or MMIO callback;
guest backing has no separate cache or buffered writes to flush.

## CPU backing image

Use `CpuState::to_bytes` and `CpuState::from_bytes` to exchange the `cpu` member
of `ExecutionContext`. Copy exactly `CpuState::BYTE_LEN` bytes at
`offset_of!(ExecutionContext, cpu)`, preserving the surrounding execution controls.
Conversion is explicitly little endian and preserves reserved bytes, inactive flag
payloads, raw segment fields, x87 and XMM encodings. The image is
`CpuState::BYTE_LEN` (488) bytes:

| Byte offset | Contents |
| ---: | --- |
| 0 | 12-byte `StoredStatusSource`: kind, three reserved bytes, two u32 operands. |
| 12 | 16-byte `FlagBytes`: CF, PF, AF, ZF, SF, OF, TF, DF, NT, AC, ID, IF, IOPL, three reserved bytes. |
| 28 | Eight u32 registers: EAX, ECX, EDX, EBX, ESP, EBP, ESI, EDI. |
| 60 | u32 EIP. |
| 64 | Six 16-byte segment records: ES, CS, SS, DS, FS, GS. |
| 160 | Eight reserved bytes. |
| 168 | u64 completed-instruction count and virtual timestamp counter. |
| 176 | Twelve-byte `StoredX87Control`: six mask bytes, PC, RC, IC, padding byte, u16 reserved bits. |
| 188 | u16 full x87 tag word. |
| 190 | u16 last x87 opcode. |
| 192 | Two u32 x87 pointer offsets: instruction, data. |
| 200 | Two u16 x87 pointer selectors: instruction, data. |
| 204 | Fourteen-byte `StoredX87Status`: IE, DE, ZE, OE, UE, PE, SF, TOP, C0, C1, C2, C3, ES, B. |
| 218 | Six reserved bytes. |
| 224 | Eight 16-byte physical x87 register slots, R0 through R7. |
| 352 | u32 packed MXCSR. |
| 356 | Four reserved bytes. |
| 360 | Eight 16-byte XMM slots, XMM0 through XMM7, low byte first. |

`StoredX87Control` keeps each exception mask in bit 0 of its named byte: invalid,
denormal, zero divide, overflow, underflow and precision. A set mask suppresses
exception delivery. `precision_control` and `rounding_control` use bits 0–1;
`infinity_control` uses bit 0 and has no execution meaning on the Pentium 4.
`reserved_bits` retains control-word positions 15–13 and 7–6 for readback; it
contains no active control fields. FLDCW unpacks these fields and FNSTCW packs
them. Their snapshot padding is preserved by both instructions and FNINIT.

`StoredX87Status` stores independent bytes rather than a packed status word.
`invalid`, `denormal`, `zero_divide`, `overflow`, `underflow`, `precision` and
`stack_fault` each use bit 0 for IE, DE, ZE, OE, UE, PE and SF respectively. `top` uses
bits 0–2; `c0`, `c1`, `c2`, `c3`, `error_summary` and `busy` use bit 0.
Serialization preserves the unused bits of these bytes without interpretation.

Each x87 slot contains a u64 significand, a u16 sign/exponent word and six
reserved bytes. The ten value bytes retain the binary80 encoding, including its
explicit integer bit and noncanonical encodings. `status.top` maps
logical ST(i) to these physical registers. The full tag word retains each
register's two-bit tag; marking a register empty does not erase its payload.
`StoredX87` is a host snapshot layout, not an FSAVE or FXSAVE memory operand.

Each segment record contains a u32 base, u32 inclusive byte limit, u16 visible
selector and six bytes: usable, readable, writable, executable, expand-down and
D/B (`big`). Constructors and resolver callbacks use 0 or 1 for boolean fields;
snapshot conversion preserves every byte verbatim.
These are loaded caches, not packed GDT/LDT descriptors.

The flag image is storage, not architectural EFLAGS. Status-source kind zero reads
the low bits of the six stored status bytes. Subtraction kinds 1/5/9, addition
kinds 2/6/10 and logic kinds 3/7/11 derive byte/word/dword status flags from the
payload. Arithmetic stores its original zero-extended operands; logic stores its
result in the first operand. The stored status bytes can be stale while a source
is active. Control and system flags always use their stored bytes. IF uses bit 0
of `if_`; IOPL uses bits 0–1 of `iopl`. Other bits are retained by snapshot conversion.
Valid source kinds are an internal invariant; a host setting concrete status flags must also
select kind zero.

Protected-mode architectural flag transfers use a fixed CPL3/IOPL0 model with IF set and
VM/RF/VIF/VIP clear. Word stack images restore the represented low flags;
dword images also restore AC and ID. IRETD restores RF on hardware, but RF is
unrepresented here along with debug delivery. Real16 transfers also read and write
IF and IOPL. TF, IF and AC are stored without enabling debug traps, interrupt
delivery or alignment checks.

```rust
use relocade_x86::CpuState;

let mut cpu = CpuState::default();
cpu.registers.eax = 42;
cpu.eip = 0x1000;
let image = cpu.to_bytes(); // Copy to the execution context's `cpu` member.
assert_eq!(CpuState::from_bytes(image), cpu);
```

`CpuState::default()` installs flat segment caches with zero visible selectors
and initializes x87 control fields to represent `037F`, status to zero and the
tag word to `FFFF` (all eight registers empty).
Other fields are zero. It is a host execution configuration, not a processor
reset or a segment-load operation. `filled` and `from_bytes` preserve literal
images; an all-zero image has unusable segment caches. Hosts using protected-mode far CALL/RET must initialize CS with a valid return
selector and provide its descriptor.

## x87 environment and stack

The implemented controls are FNINIT, FNCLEX, FLDCW, FNSTCW, FNSTSW (memory and AX),
FNOP and standalone FWAIT. The obsolete encodings DB E0 (FNENI), DB E1 (FNDISI)
and DB E4 (FSETPM) are integer no-ops on the P4 target: they preserve all x87
state and do not wait for pending exceptions. The FENI/FDISI assembler spellings
may include a separate 9B WAIT, which still checks pending exceptions.
Stack operations include FLD ST(i), FST/FSTP ST(i), FXCH,
FFREE, FINCSTP and FDECSTP. FCHS and FABS change only the sign of a nonempty
ST(0), preserving its exact encoding, including NaN payloads. Empty operands
use the ordinary masked or unmasked stack-underflow response. Memory data
transfers support FLD m32/m64/m80,
FILD m16/m32/m64 integers, FIST m16/m32, FISTP m16/m32/m64, FST m32/m64 and
FSTP m32/m64/m80. FLD1, FLDZ, FLDPI, FLDL2T, FLDL2E, FLDLG2 and FLDLN2 push
the x87 constants. They obey RC at extended precision independently of PC,
without raising precision exceptions or setting C1 for rounding. A full stack
uses the ordinary masked or unmasked stack-overflow response.
Register arithmetic supports FADD/FADDP, FSUB/FSUBP,
FSUBR/FSUBRP, FMUL/FMULP, FDIV/FDIVP and FDIVR/FDIVRP. FADD, FSUB, FSUBR, FMUL,
FDIV and FDIVR also accept binary32/64 memory sources. There is no FIST m64 or
FST m80 encoding. Comparisons support FCOM/FCOMP with register or binary32/64
memory sources, FCOMPP, FUCOM/FUCOMP/FUCOMPP and FTST. FIADD, FIMUL, FISUB,
FISUBR, FIDIV and FIDIVR accept signed m16/m32 integers. Integer sources expand
exactly before arithmetic; PC and RC apply to the result. These forms retain
TOP and use the same result, exception and native PC53 paths as real arithmetic.
FRNDINT, FSCALE, FXTRACT and FSQRT are also supported.
FPREM and FPREM1 replace ST(0) with the exact remainder of division by ST(1).
FPREM truncates the quotient toward zero; FPREM1 chooses its nearest integer,
with ties to even. Both ignore PC and RC. A complete reduction clears
C2 and places quotient-magnitude bits Q2/Q1/Q0 in C0/C3/C1. Exponent gaps of 64
or more use a 32-bit partial reduction and set C2; repeat the instruction until C2
clears. Partial reductions preserve the otherwise undefined quotient condition
codes. Neither ST(1) nor TOP changes. Exact tiny remainders raise underflow only
when unmasked, writing the exponent-adjusted result without a precision exception.
The JIT retains normal/zero inputs with a normal divisor and an in-range result;
other cases restart in the interpreter before the instruction has effects.
F2XM1 computes 2^ST(0)-1 for finite inputs in [-1, 1]. It uses extended
precision under RC independently of PC, preserves signed zero, and handles
denormal inputs and masked or unmasked underflow and precision exceptions.
The numerical kernel uses two binary64 components with a separate extended
exponent. Normal inputs with an in-range result stay compiled; exceptional
operands and tiny results restart in the interpreter before current effects.
Outside the specified finite input domain, retaining the source is local policy,
except that negative infinity returns -1 without an exception. This infinity
response matches observed native behavior where Intel leaves the result undefined.
FYL2X computes ST(1) * log2(ST(0)), retaining extra significand precision through
the multiplication before rounding under RC and popping the stack. PC is ignored.
The normal finite calculation reports precision for mathematically nonzero results.
C1 comes from final rounding, without reproducing processor-specific approximation
biases. Zero, infinity and NaN responses follow the architectural operand table.
FYL2XP1 similarly computes ST(1) * log2(1+ST(0)) and pops. It factors out the
increment so tiny values survive even when 1+x would round to one. Its specified
input domain is symmetric: |x| <= 1-sqrt(2)/2. Outside that finite domain,
retaining ST(1) and completing the pop is local policy; NaNs and unsupported
encodings still use their ordinary operand responses.
FPATAN computes atan2(ST(1), ST(0)) with all four quadrants, signed-zero and
infinity combinations, then replaces ST(1) and pops. Separate operand exponents
preserve tiny ratios; an omitted negative atan tail still influences directed
rounding. Its 0/0 and infinity/infinity cases are defined angles, not division
exceptions. PC is ignored and RC controls the final extended-precision result.
FSIN and FCOS reduce finite inputs with |x| < 2^63 using the P4's 66-bit pi
approximation, then evaluate a shared bounded series and round under RC at full
extended precision. Large finite operands remain unchanged with C2 set; in-range
execution clears C2. C1 is cleared for out-of-range inputs, where it is undefined.
Infinity is invalid. Tiny sine results retain the extended underflow response.
FSINCOS performs the same reduction once, leaves sine in the original slot and
pushes cosine. Each component rounds independently; the cosine rounding increment
sets C1. Unmasked operand exceptions suppress both writes and the push, while unmasked
range or precision exceptions publish both results before deferred delivery.
Out-of-range execution preserves both payloads, their tags and TOP. FXTRACT uses
the same shared two-result publication and stack-fault response.
FPTAN leaves tangent in the original slot and pushes one for ordinary operands.
It divides unrounded sine/cosine components, retaining exact reciprocal evidence
near a pole and the positive tangent tail for tiny operands. Its C1 comes from
tangent rounding. NaNs or masked invalid operands produce exceptional values in
both slots. Range exits and exception publication share the FSINCOS behavior.
FXAM reports ST(0)'s numerical class in C0/C2/C3 and its sign in C1. Empty
entries report the empty class and retain their payload's sign; examination
never raises a numerical exception or stack fault, including for signaling
NaNs and unsupported encodings. Register contents, tags and TOP are unchanged.
Execution assumes an enabled FPU with native exception reporting, corresponding
to CR0.EM=0, CR0.TS=0 and CR0.NE=1. CR0 and device-not-available exceptions are not
modeled by this user-mode environment.

FWAIT and FLDCW observe `status.error_summary`, the architectural ES bit. A pending
exception returns #MF at that waiting instruction's EIP without retirement.
The stored x87 instruction and data pointers retain the previous operation;
they are not replaced by the waiter's address. Hosts supplying x87 state must
keep ES/B consistent with its sticky exception flags and control masks.

FLDCW first checks for a pending exception, then reads its two-byte operand.
A successful load updates the control word and refreshes ES/B from the retained
exception flags and new masks. Newly unmasked exceptions become pending; the
load itself completes and the next waiter reports #MF. The pending check before
a simultaneously faulting operand is this implementation's ordering policy;
Intel does not specify a universal priority for those competing execution faults.

The no-wait controls bypass pending-exception delivery. FNINIT resets the x87
environment while preserving raw register payloads and snapshot padding.
FNCLEX clears exception flags, stack fault, ES and B; this implementation retains
the otherwise undefined condition codes. Control/status stores use two bytes
regardless of the operand-size prefix. FNSTSW AX preserves EAX's upper half.
FNOP checks for a pending exception, then records its instruction pointer and
opcode without changing the stack or status. This implementation retains its
undefined condition codes. The other control instructions leave the x87
pointers and opcode unchanged.
Reserved control bits have no execution meaning; their readback is not a
Pentium 4 compatibility guarantee.

Waiting spellings such as FINIT and FSTCW encode a separate `9B` FWAIT followed
by the no-wait instruction. They retain separate retirement and fault boundaries.
The behavior above follows Intel's Pentium-4-era manuals:
[Volume 1](https://kib.kiev.ua/x86docs/Intel/SDMs/253665-014.pdf), sections
8.1.7–8.1.8 and 8.3.12, and the control-instruction entries in
[Volume 2A](https://kib.kiev.ua/x86docs/Intel/SDMs/253666-014.pdf).

Extended transfers preserve all ten value bytes, including signaling NaNs,
pseudo-denormals and unsupported encodings; these movements do not perform
arithmetic or rounding. Nonempty destination tags describe the stored encoding.
Physical-slot padding stays with its storage location.
FLD ST(i) reads its source relative to the old TOP, before pushing. FSTP ST(i)
writes its destination before marking the old stack top empty and advancing TOP.
FFREE changes only its register tag; rotations change TOP without moving payloads.

FLD m32/m64 expands the source exactly, independently of precision and rounding
control. Subnormals become normal extended values and set DE. A masked SNaN sets
IE and becomes a QNaN with its sign and payload preserved; an unmasked SNaN
suppresses the push. Stack overflow takes priority over both source conditions.
An unmasked denormal load still pushes its value and sets DE, ES and B; the next
waiting instruction reports #MF. This follows the instruction-specific
clarification in Intel's [FLD entry, Volume 2](https://cdrdv2-public.intel.com/789581/325383-sdm-vol-2abcd.pdf),
which is more specific than the older general description of unmasked
denormal-operand exceptions. These exact conversions do not set PE, UE or OE.

Arithmetic with binary32/64 memory sources expands the input exactly before
applying PC and RC to the result. The original source classification still
determines invalid and denormal exceptions, even when a narrow subnormal expands
to a normal extended value. An unmasked invalid or denormal exception suppresses
the destination update and leaves #MF pending for the next waiting instruction.

Division uses the same PC/RC rounding and deferred exception handling.
Finite nonzero values divided by signed zero set ZE; masked zero divide stores
infinity with the operands' exclusive-OR sign, while unmasked zero divide
preserves the destination and TOP. Zero divided by zero and infinity divided by
infinity set IE instead. NaN and unsupported-encoding responses precede zero
divide, and zero divide precedes a denormal-operand response. Thus a denormal
dividend divided by zero sets ZE without DE. These priorities follow Intel's
[P4 Volume 1, sections 4.9.2 and 8.5.3](https://kib.kiev.ua/x86docs/Intel/SDMs/253665-014.pdf).

Comparisons order the full extended values independently of PC and RC, publish
C0/C2/C3 and clear C1. FCOM and FTST signal invalid for any NaN; FUCOM accepts
quiet NaNs as unordered. Snapshot blocks keep comparisons without new operand
exceptions compiled, including infinities and FUCOM quiet NaNs. Operand exceptions
restart in the interpreter before comparison metadata, condition codes or pops
change. Retained binary64 operands use native Wasm comparisons; wider values keep
exact integer ordering without a conversion to binary64.

FILD converts signed 16-, 32- and 64-bit memory integers exactly, independently
of PC and RC. The opcode fixes the width even with an operand-size prefix.
Zero receives the zero tag; every other integer becomes a normal extended value.
Only stack overflow can raise a new x87 exception. A masked overflow pushes
the indefinite value; an unmasked overflow preserves TOP and the destination,
setting IE, SF, C1, ES and B for deferred delivery. See Intel's
[FILD entry, Volume 2A](https://cdrdv2-public.intel.com/812383/253666-sdm-vol-2a.pdf).

FIST/FISTP round the original extended value to a signed integer under RC,
independently of PC, and test the rounded result against the destination range.
NaNs, infinities, unsupported encodings and results outside that range raise IE.
Masked invalid stores the destination's most-negative integer; unmasked invalid
preserves memory and TOP. Integer stores do not raise DE, OE or UE. Valid
inexact conversions set PE and use C1 to report an increase in magnitude; they
complete the store and FISTP pop even when precision is unmasked. FIST preserves
the full source value. Operand size is fixed by the opcode. See Intel's
[FIST/FISTP entry, Volume 2A](https://cdrdv2-public.intel.com/812383/253666-sdm-vol-2a.pdf)
and [Volume 1, section 8.5.6](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366521.pdf).

FST/FSTP m32/m64 round the current extended value directly to the destination
under RC, independently of PC. Signed zeros and infinities retain their signs;
NaNs retain their sign and high payload bits. SNaNs set IE and are quieted when
masked. Unsupported encodings set IE and produce negative indefinite when masked.
An unmasked invalid exception suppresses the store and pop. True and pseudo
denormals take underflow responses without setting DE.

Range detection rounds to destination precision with an unbounded exponent.
The masked subnormal result is rounded separately from the original value, so
there is no extra rounding. A store can therefore produce minimum normal with
UE and PE set. Masked underflow sets UE only when tiny and inexact; unmasked
underflow reports exact tiny values too. Masked overflow returns signed infinity
or maximum finite according to RC. Unmasked overflow or underflow suppresses
the store and pop, clears C1 and does not newly set PE. Precision exceptions
commit the rounded store and pop even when unmasked, leaving #MF pending for
the next waiter. C1 records an increase in magnitude, including the infinity
response to masked overflow. These rules follow the P4 FST/FSTP entry and
Volume 1 sections 8.5.4–8.5.6; the unbounded-exponent wording is explicit in
[Intel Volume 1, section 8.5.5](https://cdrdv2-public.intel.com/874241/253665-090-sdm-vol-1.pdf).

Stack faults set IE and SF. C1 distinguishes overflow from underflow; a source
underflow takes priority over a destination overflow. Masked faults substitute
the extended indefinite value and perform the instruction's push or pop.
For FXCH, each empty source is replaced before the two values are exchanged.
An unmasked stack fault suppresses data and stack changes, records pending
status, and retires its producer. Integer and no-wait instructions can continue;
the next waiting instruction reports #MF. Reporting it does not clear the status.

The complete memory operand passes segment checks and, in protected mode, page
checks before an x87 data instruction changes data, stack state or numerical
pointers. This implementation
checks these accesses before generating a new stack or source exception. A
preexisting pending exception is checked first. Faulting stores do not write an
earlier portion of the value or pop the stack.

Stack, arithmetic, comparison and data-transfer instructions record their
instruction offset, selector and opcode. Memory forms also record their effective offset and segment selector;
register forms preserve the otherwise undefined data pointer. Opcode recording
uses the Pentium 4 compatibility-mode policy, keeping the last x87 opcode valid
after every such instruction. The opcode fixes these memory operands' widths;
address-size and segment prefixes retain their ordinary meaning.

Status fields remain separate in generated execution and in the host snapshot.
FNSTSW assembles the architectural status word when requested; exits publish
changed fields directly. Untouched imported fields retain every byte, including
unused bits and ES/B. Changing a field may normalize that field's unused bits.

## Entry validity

The host must admit each entry against `CompiledModule::execution_target` before
executing it, including when following a direct dispatch link. This replaces the
former profile-only metadata. `ExecutionTarget::is_compatible_with(active, segments)`
requires exact target equality and checks the current segment caches. The host
supplies its actual active target; segment bytes alone cannot establish the mode.

Use the complete target in cache and direct-link keys, and require the same target
for interpreter fallback. A model with additional enabled capabilities is not an
interchangeable target: instruction availability is part of the guest contract.
Changing the target requires rebuilding or selecting matching entries and links.

`ExecutionProfile` selects `Protected(SegmentProfile)` or `Real16` at compilation
time. `SegmentProfile` supplies protected-mode code defaults and segment assumptions.

| Profile | Compilation assumptions |
| --- | --- |
| `Protected(Flat32)` | Usable, flat readable CS and writable expand-up DS/ES/SS; zero bases, full u32 limits, CS.D=1 and SS.B=1. FS/GS are runtime inputs. |
| `Protected(Segmented32)` | CS.D=1; segment access and SS.B are checked or read at runtime. |
| `Protected(Segmented16)` | CS.D=0; segment access and SS.B are checked or read at runtime. |
| `Real16` | Ordinary real mode with canonical caches: selector × 16 bases, FFFF limits, 16-bit CS/SS defaults; the physical map controls A20. |

`ExecutionProfile::is_compatible_with` tests those assumptions. Selectors and ordinary
DS/ES D/B bits do not determine flat compatibility.
Segmented profiles can admit unusable caches or restrictive limits so execution
can report their guest faults. All `Protected` entries execute at CPL3, including
those with 16-bit code defaults. The host selects the execution mode; cache
compatibility alone cannot distinguish it. The profile is part of the complete
execution target described above.

Segment fields stay fixed until the terminal segment load. Each field is read
lazily and reused within its compilation path; profile defaults become constants
when a segment is first requested. Address defaults retain a DS/SS hint so common
constants can be reused when the base register is decoded at runtime. Other
dynamic indices load the selected backing field. Known permissions and full limits
skip range-check construction, including ordinary Flat32 accesses.
The selected profile must remain valid until a terminal segment load commits its
cache. Only state publication and dispatch follow that commit; the next entry
must be admitted against the new state.

Snapshot blocks check their complete compiled fetch span against current CS at
entry, before any instruction effects. This includes the last byte of the final
compiled instruction; bytes after the compilation boundary are irrelevant. A failed
check tail-calls `relocade.interpret` at the block's starting EIP without retiring any
instructions. The interpreter executes any valid prefix and reports the first
architectural fault, which may precede the invalid CS fetch. Checks proved by the
profile fold away. This guard needs no observed CPU snapshot and accepts any CS
permissions and limit that allow the span.

The host must still ensure that snapshot bytes match readable guest memory at
CS.base + each instruction's EIP, with 32-bit offset and linear wrapping. The guard
does not check code bytes or mappings, nor establish profile compatibility.

The host must preserve validity during execution and revalidate or invalidate
affected entries and links when relevant CS state, code bytes or mappings change.
An instruction changing a relied-upon assumption must end the block. There is no
code cache or automatic invalidation in these libraries. A snapshot producer that
cannot read a complete instruction can stop at the valid prefix and enter the
interpreter at the next EIP to handle the fetch precisely.

Protected-mode execution may reuse successful data-access checks while mappings
remain stable. Each access still checks its segment and transfers current guest
bytes; retained checks never cache guest contents or survive an entry's dispatch.

The interpreter checks required instruction bytes through CS and the backing map
at runtime. The protected-mode direct decoding loop may retain the current
page-table entry until it leaves
that decoder invocation. It still fetches live guest bytes and checks every access;
no mapping cache survives a host dispatch or a new interpreter entry. The host may
therefore remap code pages between entries under the existing validity rules.
CS checks precede mapping checks. Real16 uses direct RAM/ROM windows and exact
required-byte reads when a direct window is unavailable. Physical routing is
rechecked after each instruction, including after MMIO callbacks that remap code. It fetches all fields of a supported
form before data access, but rejects an unsupported group extension before reading
unneeded fields. Requiring byte sixteen raises #GP(0); an earlier unavailable
required byte faults first. It never fetches a branch destination as part of the
transferring instruction.

## Ordinary real mode

Select `ExecutionProfile::Real16` and initialize caches with `Segments::real_mode()`.
Use `StoredSegment::real_mode(segment, value)` for a nonzero segment register.
This configuration is host-selected execution, not a processor reset or an emulated
CR0 transition. The CPU backing layout is unchanged.

```rust
use relocade_x86::{CpuState, ExecutionProfile, Segment, Segments, StoredSegment};

let mut cpu = CpuState::default();
cpu.segments = Segments::real_mode();
cpu.segments.cs = StoredSegment::real_mode(Segment::Cs, 0x1234);
assert!(ExecutionProfile::Real16.is_compatible_with(&cpu.segments));
```

All sixteen segment-value bits participate in `value << 4`; zero is valid and
there are no selector privilege checks or descriptor callbacks. Loads install
canonical caches. This is valid because the profile already requires canonical
limits and attributes; it does not model retained hidden state after switching
from protected mode. Mode transitions and unreal-mode caches remain unsupported.

Operand/address overrides select 32-bit operands/addresses while SS still uses SP.
Every transferred span must fit the 64 KiB segment: SS failures report #SS(0),
others #GP(0). Real-mode type permissions allow data writes through CS overrides.
The physical map disables A20 by default for DOS compatibility, clearing address
bit 20 before RAM, ROM or MMIO routing. The host can enable it through
`set_a20_enabled(true)`; see [physical memory](physical-memory.md). Far JMP/CALL/RET
commit CS and dispatch, with target fetch belonging to the next entry. Real CALL
checks the transferred offset plus selector; a dword selector slot reserves four
bytes but transfers only its low word. RET checks both complete stack slots.

PUSHF/POPF transfer word or dword images through SS:SP. Real16 POPF restores IF
and IOPL regardless of their entry values; only dword transfers change AC and ID.
Reserved and unrepresented image bits are ignored.

CLI/STI clear or set IF in Real16 and raise #GP(0) in protected profiles, which
fix CPL3/IOPL0. IF changes are immediately visible to PUSHF. External interrupt
delivery and STI's one-instruction inhibition are not modeled; IF alone must not
be used by the host to decide when to inject interrupts.

IRET/IRETD check all three stack slots before restoring IP/EIP, CS and FLAGS.
IRETD rejects an EIP above `0xffff` before reading the CS and FLAGS fields. The
return commits all state and dispatches; target fetch belongs to the next entry.
Entry NT does not select a task return in real mode. Word returns preserve AC/ID;
dword returns restore them, with RF still unrepresented as described above.

INT imm8, INT3 and taken INTO use the conventional 256-entry interrupt vector
table at linear address zero. Each entry contains a word IP followed by a word
CS. IDTR relocation and shortened table limits are not modeled. Entry checks the
complete six-byte stack frame, pushes FLAGS, CS and the following IP, then loads
the vector through physical-memory routing. Operand/address/segment overrides do
not alter these word transfers. Stack writes precede vector reads, including when
the stack aliases the table. IF does not mask software interrupts. Entry clears
IF, TF and AC, preserves other flags and dispatches with the new CS:IP; target
fetch belongs to the next entry. INTO with OF clear dispatches to its fallthrough
without touching the stack or vector table.

Protected INT/INT3/INTO delivery returns the unsupported-instruction exit before
effects. The forms are decoded before that exit, so INT requires its immediate
byte in both modes. This stage does not add protected interrupt gates or host
interrupt injection.

ARPL/LAR/LSL/VERR/VERW raise #UD before operand access and terminate snapshot
compilation. Both decoders read the selected form's physical encoding before
mode rejection.
PUSHA/PUSHAD honor the specified #GP for low odd SP values 7 through 15. Stack
faults are reported to the host; exception delivery, double faults and shutdown
are outside this execution model. ENTER checks actual transfers without requiring
backing for its unused allocation; Real16's SP always fits its canonical SS.

These rules follow Intel's Pentium 4 [Volume 3, §15.1 and §9.9.2](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366821.pdf)
and the instruction entries in [Volume 2A](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366621.pdf)
and [Volume 2B](https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366721.pdf).
RET's two-slot capacity follows the corrected [Volume 2 RET entry](https://cdrdv2-public.intel.com/774492/325383-sdm-vol-2abcd.pdf).

## Segment resolver

Protected-mode segment loads call `relocade.resolveSegment(segment: i32, selector: i32)`, returning
eleven Wasm i32 results: `(status, error_code, base, limit, selector, usable,
readable, writable, executable, expand_down, big)`.
Segment indices are ES=0, CS=1, SS=2, DS=3, FS=4 and GS=5. Selector inputs and
selector outputs use zero-extended 16-bit values. The six property results are
booleans, represented by 0 or 1.

Status zero supplies a complete normalized `StoredSegment`. Failure statuses are
architectural vectors 11 (#NP), 12 (#SS) or 13 (#GP); only their error code is used.
Unknown statuses or invalid success records violate the host contract. Wasm owns
the instruction's remaining checks, cache commitment, retirement and fault exit.
In particular, resolving CS does not validate a transfer's target offset.

Protected-mode RETF and IRET require a return selector with RPL 3 before resolving CS. IRET
checks all three slots and reads their values before resolving the selector.
It commits CS, EIP, flags and the stack pointer only after all checks succeed.
An entry NT flag of one requests an unsupported task return before ordinary stack
access. Restoring NT from the frame affects a later IRET, not the current return.

The callback resolves the current thread's descriptor view, for example through
`DescriptorTables::resolve_user_segment`. It must not inspect or mutate CPU state,
guest RAM or page tables, or reenter guest execution. Descriptor-table edits affect
future loads and leave already-loaded CPU caches unchanged.

`DescriptorTables` provides host-managed global/local slots keyed by selector table
and index bits, ignoring RPL for lookup. Resolution applies protected-mode CPL3
load rules; it neither allocates Windows selectors nor models guest GDTR/LDTR,
packed descriptor memory or descriptor accessed-bit writes. All represented
descriptors have A=1 and L=0. `SegmentLimit` keeps the encoded 20-bit limit and
granularity together; loaded caches still use an effective byte limit. See the
type's API documentation for load validation and descriptor construction.

## Descriptor queries

LAR/LSL/VERR/VERW call `relocade.querySegmentDescriptor(selector: i32)` after reading
their 16-bit selector operand. The input is zero-extended. The callback returns
five Wasm i32 results in this order: `(visible, readable, writable, accessRights,
limit)`. The first three results are booleans represented as 0 or 1. The results
describe code/data descriptors at CPL3:

| Result | Meaning |
| --- | --- |
| `visible` | Visible; LAR and LSL use this as ZF. |
| `readable` | Readable; VERR uses this as ZF. |
| `writable` | Writable; VERW uses this as ZF. |
| `accessRights` | LAR's 32-bit result, with the bit positions specified below. |
| `limit` | Inclusive effective byte limit, with page granularity already expanded. |

`accessRights` uses bit positions within the descriptor's upper 32 bits: type 11:8,
S 12, DPL 14:13, P 15, AVL 20, L 21, D/B 22 and G 23. All remaining bits are zero
in this model, including the architecturally undefined bits 19:16.

Null, missing and privilege-inaccessible descriptors return all zeros. Presence
affects the reported P bit, not visibility or read/write permission. Execute-only
code can be visible without either permission; code is
never writable. LAR/LSL preserve the destination on failure and truncate successful
results to the operand width. All four instructions change only ZF among flags.

The represented environment contains only code/data entries, which share LAR/LSL
eligibility. System descriptors and gates are outside this contract. In particular,
a future gate model needs separate LAR/LSL eligibility. A=1 and L=0 match the host
descriptor invariants; undefined LAR bits 19:16 are chosen as zero.

The callback queries the current thread's descriptor table, for example through
`DescriptorTables::query_user_segment_descriptor`, which returns a
`SegmentDescriptorInfo` with named booleans and values. It must not inspect or
mutate CPU state, guest RAM or page tables, or reenter guest execution. It does not
load a segment or fault for a rejected selector. Table edits affect the next query
while loaded caches remain unchanged. Guest operand reads can still fault before
the callback runs.

## Fault and host exits

A guest fault returns directly without calling dispatch. Earlier completed
instructions remain published; EIP identifies the faulting instruction, which
does not retire. Its entry CPU state is preserved except for the partial progress
described below.

Architectural checks for a complete operand precede its writes. In protected mode
this includes every page's write permission; Real16 transfers follow the physical
map's RAM/ROM/MMIO/unmapped routing. Instructions with ordered
multiple writes, such as ENTER and PUSHA, retain completed writes if a later access
faults. REP faults retain completed elements, current indices and the remaining
count, with EIP at the instruction's first prefix. REP LODS also retains the last
successfully loaded accumulator value. Repeated CMPS/SCAS faults retain
the flags from instruction entry. A successful repetition keeps the last comparison's
flags; zero-count execution preserves them. A successful REP retires once.

POPA also retains registers restored before a later fault. ESP and EIP remain at
instruction entry and the instruction does not retire. Every slot is checked
against SS and, in protected mode, paging in order, including the discarded SP/ESP slot, whose value
is not read. This models observed partial progress; Intel documents incomplete
register restoration on POPAD faults without specifying every CPU's exact pattern.

The u64 return format is `(tag << 48) | (payload << 32) | address`, with a 16-bit
tag, 16-bit payload and 32-bit address. These tags are independent of architectural
exception vector numbers:

Hosts can decode these words with `ExecutionExit::from_bits`, which returns an
`Exception`, an unsupported-instruction diagnostic, `BudgetExhausted`, or a
`HostReturn` or `Deferred` requested by a host-call function. The embedding owns
the reason for a host return. Deferred work is awaited by the runtime after the
guest call returns. Unknown tags and nonzero reserved fields return `None`.
Host-defined values returned by `dispatch` or `interpret` must be distinguished
before decoding; either callback may return any i64. Track callback results or
use a distinct result convention. If a callback
continues guest execution, classify the resulting execution outcome according to
that callback's contract. A matching exit word alone does not establish its source.

| Exit | Tag | Payload | Address |
| --- | ---: | --- | --- |
| Divide error | 1 | Zero | Zero |
| General protection | 2 | Error code | Zero |
| Page fault | 4 | Error code | First denied linear byte |
| Unsupported instruction | 8 | Diagnostic opcode byte | Instruction's starting EIP |
| Stack fault | 16 | Error code | Zero |
| Segment not present | 32 | Error code | Zero |
| BOUND range exceeded | 64 | Zero | Zero |
| Invalid opcode | 128 | Zero | Zero |
| Floating-point error | 256 | Zero | Zero |
| SIMD floating-point exception | 512 | Zero | Zero |
| Deferred host call | 8192 | Zero | Zero |
| Host return | 16384 | Zero | Zero |
| Instruction budget exhausted | 32768 | Zero | Zero |

Floating-point error reports #MF (architectural vector 16). Its return value
contains no payload; the published x87 status and environment describe the
pending exception.

SIMD floating-point exception reports #XM (architectural vector 19) at the
faulting instruction, without retirement. The published MXCSR includes newly
raised sticky exception flags; the instruction's XMM or EFLAGS destination is
preserved. Existing sticky flags do not themselves trigger an exception.

UD2 raises invalid opcode (#UD, architectural vector 6) after its complete encoding
has been fetched. It preserves CPU state and memory, does not retire or dispatch,
and leaves EIP at its first prefix or opcode byte. Snapshot compilation ends at
UD2 without decoding subsequent bytes.

Page-fault error bit 0 means a present but denied page, bit 1 means a data write,
and bit 4 means instruction fetch. Cached segment-access failures use #SS(0) for
SS and #GP(0) for other segments. Resolver faults retain their selector error code.

An unsupported exit describes an encoding or execution path outside the
implementation's subset, not an architectural invalid-opcode exception. Its
diagnostic byte is the first byte after size and segment prefixes, `0F` for an
extended opcode, or the selected `F0`/`F2`/`F3` prefix for an unsupported prefixed
form. It does not retire or dispatch. Protected IRET with entry NT set reports
`CF` at the instruction's restart EIP, including its prefixes. Snapshot
construction reports `BlockError` for unsupported encodings; state-dependent
unsupported paths remain
runtime exits. Truncated byte input alone cannot establish a guest fetch fault.

Faults are reported to the host. Automatic delivery of guest faults, protected
interrupt gates, privilege transitions, external interrupt/debug delivery, mode
transitions and SS-load inhibition are not modeled. Native Wasm traps from broken
backing or internal arithmetic invariants are implementation errors, outside this
guest-fault protocol.

## MMX state and integer operations

MM0–MM7 alias the significands of physical x87 slots R0–R7, independent of TOP
and the tag word. There is no separate MMX register array or mode flag in the
CPU image. The execution profiles fix CR0.EM/TS clear; the CPU model separately
selects the available MMX, SSE and SSE2 instruction extensions.

MOVD and MOVQ transfer raw integer bits. MOVD uses a 32-bit GPR or four memory
bytes even in 16-bit code; loads into MMX clear the upper 32 bits. MOVQ uses eight
memory bytes. Both accept unaligned addresses. A register destination receives
sign/exponent `ffff`; a memory or GPR destination preserves all x87 payloads.
Successful transfers mark all tags valid and reset TOP to zero. EMMS marks all
tags empty and resets TOP to zero while retaining every payload and exponent.

PAND, PANDN, POR and PXOR operate on all 64 bits of an MMX register and a
register or unaligned eight-byte memory source. PANDN computes `(~destination)
& source`. Logical operations have the same tag, TOP and destination exponent
effects as MOVQ, including same-register operations such as `PXOR mm0, mm0`.

PADDB/PADDW/PADDD and PSUBB/PSUBW/PSUBD add or subtract independent byte,
word or doubleword lanes. Results wrap at the lane width without saturation or
cross-lane carries and borrows. They accept register or unaligned eight-byte
memory sources and have the same MMX environment effects as logical operations.
The compiler lowers packed arithmetic to standard WebAssembly SIMD operations.

PCMPEQB/PCMPEQW/PCMPEQD compare lanes for equality; PCMPGTB/PCMPGTW/PCMPGTD
compare signed lanes for greater-than. Each destination lane becomes all ones
when the comparison is true and zero otherwise. These instructions use the same
register/eight-byte memory forms and MMX environment effects as packed arithmetic.

MMX instructions, including EMMS, deliver a pending x87 exception before operand
access. A memory fault preserves the current instruction's destination, tags and
TOP. These instructions preserve EFLAGS, MXCSR, x87 control and other status
fields, and the saved x87 opcode and pointers. Hosts should execute EMMS before
resuming x87 stack operations after MMX use.

## SSE state and raw transfers

`CpuState::simd` contains `StoredSimd { mxcsr, reserved, xmm }`. Default state has
MXCSR `0x1f80` and zero XMM registers. Literal snapshots retain every bit, including
reserved MXCSR bits and padding; these bytes are not an FXSAVE image. These
fields belong to the CPU snapshot within `ExecutionContext`.

The execution profiles fix CR0.EM/TS clear and CR4.OSFXSR/OSXMMEXCPT set, enabling
SIMD state access and exception delivery. These control-register assumptions are
independent of the CPU model's instruction admission. MOVUPS, MOVUPD,
MOVAPS, MOVAPD, MOVDQA, MOVDQU, MOVSS, MOVSD, XORPS and XORPD preserve raw bits
and leave MXCSR, EFLAGS and x87 state unchanged. MOVUPS/MOVUPD/MOVDQU accept
unaligned memory. MOVAPS/MOVAPD/MOVDQA and legacy XORPS/XORPD require 16-byte
linear alignment, checked after segment validation and before page translation.
Scalar MOVSS/MOVSD memory operands access exactly four/eight bytes and permit
unaligned addresses. Loads clear the rest of the destination XMM register;
register copies preserve its upper bits. Stores write only the low scalar lane.

Both decoders select mandatory 66/F2/F3 by prefix presence, independently of the
code segment's default operand size. Unsupported mandatory-prefix combinations do not
select an unprefixed SSE form. XMM state uses the same retained-field mechanism
as integer state, including runtime-indexed register accesses and fault publication.
The complete operand span is checked before transfer. Scattered virtual backing
and physical MMIO use ordered integer parts; contiguous packed transfers use Wasm SIMD.
Hosts must enable the standard WebAssembly SIMD feature for these instructions.

LDMXCSR/STMXCSR transfer an unaligned four-byte architectural MXCSR word.
`StoredSimd::MXCSR_MASK` is `0x0000ffff`: this target supports DAZ (bit 6), in
addition to exception status, exception masks, rounding control and FTZ. DAZ was
optional on Pentium 4; hosts can use this mask as the emulator's declared policy.
`StoredSimd::MXCSR_RESET` is `0x1f80`. LDMXCSR raises #GP(0) if any bit outside
the supported mask is set, after completing the memory read and before changing
state. Loading exception flags or unmasking them does not itself raise a SIMD
floating-point exception. STMXCSR writes supported bits and zeroes reserved bits
without normalizing the literal CPU snapshot. Both instructions leave XMM, EFLAGS
and x87 state unchanged. Floating-point arithmetic and conversions use the same
MXCSR controls and SIMD exception boundary.

COMISS/COMISD and UCOMISS/UCOMISD compare the low binary32/binary64 lane and
write ZF/PF/CF, clearing OF/SF/AF. COMI raises invalid for any NaN; UCOMI raises
invalid only for a signaling NaN. A NaN response suppresses the denormal condition
in that comparison. DAZ treats subnormal inputs as signed zero; otherwise they
raise the denormal condition. Unmasked conditions publish MXCSR and deliver #XM
before changing EFLAGS. Memory sources access exactly four/eight unaligned bytes,
and access faults precede numerical exceptions. XMM and x87 state are preserved.

CMPSS/CMPSD and CMPPS/CMPPD implement the eight legacy predicates selected by
immediate bits 2:0. True lanes become all ones and false lanes become zero. Scalar
forms preserve the other destination lanes; packed forms replace all lanes and
require aligned 16-byte memory operands. The LT, LE, NLT and NLE predicates signal
invalid on quiet NaNs; every predicate signals on signaling NaNs. Packed operations
collect exception conditions from all lanes before updating MXCSR and checking
for #XM. They preserve EFLAGS and the entire XMM destination on an unmasked fault.

MINSS/MINSD/MINPS/MINPD and MAXSS/MAXSD/MAXPS/MAXPD use the same scalar/packed
access and exception boundary. Equal values (including opposite zero signs) and
either NaN select the second operand. Every NaN raises invalid; when masked, a
selected signaling NaN retains its original bits. DAZ converts subnormal inputs
to signed zero, including a selected second operand paired with a NaN. FTZ and
rounding control do not change these selection operations. They preserve EFLAGS.
