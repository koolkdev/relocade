# wasm86

Rust components for x86 execution in WebAssembly.

`wasm86-x86` compiles MOV, ADD, ADC, SUB, SBB, CMP, AND, OR, XOR, TEST,
INC, DEC, NEG, NOT and SETcc blocks from byte snapshots:

```rust
let block = wasm86_x86::compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
```

The requested instruction count is exact. Missing, overlong or unsupported
selected instructions are construction errors; bytes after the selection are ignored. The returned
module exports `block_1000` and imports `wasm86.cpuState` memory (minimum one
64-KiB page) and `wasm86.dispatch(i32) -> i64`.

`CpuState` exposes the backing state as plain Rust fields. `Registers`,
`StoredFlags` and `StatusFlags` retain every field, including reserved bytes and
inactive flag data. The types support copying and full equality comparisons:

```rust
let mut cpu = wasm86_x86::CpuState::default();
cpu.registers.ebx = 0x1234_5678;
cpu.eip = 0x1000;
let bytes = cpu.to_bytes();
assert_eq!(wasm86_x86::CpuState::from_bytes(bytes), cpu);
```

Copy `cpu.to_bytes()` into the start of the imported CPU memory to initialize it.
Decode its first `CpuState::BYTE_LEN` bytes with `CpuState::from_bytes` to inspect
the result. Conversion is explicitly little endian on every host and preserves
noncanonical flag bytes without interpreting them. The backing image is 152 bytes:
EAX through EDI are dwords in encoding order at offsets 24–52, EIP is at 56, and the
completed-instruction count is at 144. `Registers` also supports indexing by
`Gpr32` for cases that select a register dynamically.

An exit publishes its current flag source, then
dirty registers in first-write order. EIP and count use 32-bit wrapping arithmetic.
The number of completed instructions is fixed during compilation for each exit.
An exit with progress reads the runtime counter and adds that number; an exit
with no progress leaves it unchanged. The block
tail-calls dispatch with the next EIP and returns its result. This snapshot path
supports these forms in default-32 operand and address mode:

| Operands | Byte | Word (`66`) | Dword |
| --- | --- | --- | --- |
| MOV opcode-selected register and immediate | B0–B7 | B8–BF | B8–BF |
| MOV register and register/memory | 88/8A | 89/8B | 89/8B |
| MOV register/memory destination and immediate | C6 /0 | C7 /0 | C7 /0 |
| MOV accumulator and absolute memory offset | A0/A2 | A1/A3 | A1/A3 |
| ADD register and register/memory | 00/02 | 01/03 | 01/03 |
| ADD accumulator and immediate | 04 | 05 | 05 |
| ADD register/memory and immediate | 80 /0 | 81/83 /0 | 81/83 /0 |
| ADC register and register/memory | 10/12 | 11/13 | 11/13 |
| ADC accumulator and immediate | 14 | 15 | 15 |
| ADC register/memory and immediate | 80 /2 | 81/83 /2 | 81/83 /2 |
| SUB register and register/memory | 28/2A | 29/2B | 29/2B |
| SUB accumulator and immediate | 2C | 2D | 2D |
| SUB register/memory and immediate | 80 /5 | 81/83 /5 | 81/83 /5 |
| SBB register and register/memory | 18/1A | 19/1B | 19/1B |
| SBB accumulator and immediate | 1C | 1D | 1D |
| SBB register/memory and immediate | 80 /3 | 81/83 /3 | 81/83 /3 |
| CMP register and register/memory | 38/3A | 39/3B | 39/3B |
| CMP accumulator and immediate | 3C | 3D | 3D |
| CMP register/memory and immediate | 80 /7 | 81/83 /7 | 81/83 /7 |
| AND register and register/memory | 20/22 | 21/23 | 21/23 |
| AND accumulator and immediate | 24 | 25 | 25 |
| AND register/memory and immediate | 80 /4 | 81/83 /4 | 81/83 /4 |
| OR register and register/memory | 08/0A | 09/0B | 09/0B |
| OR accumulator and immediate | 0C | 0D | 0D |
| OR register/memory and immediate | 80 /1 | 81/83 /1 | 81/83 /1 |
| XOR register and register/memory | 30/32 | 31/33 | 31/33 |
| XOR accumulator and immediate | 34 | 35 | 35 |
| XOR register/memory and immediate | 80 /6 | 81/83 /6 | 81/83 /6 |
| TEST register/memory and register | 84 | 85 | 85 |
| TEST accumulator and immediate | A8 | A9 | A9 |
| TEST register/memory and immediate | F6 /0 | F7 /0 | F7 /0 |
| INC opcode-selected register | — | 40–47 | 40–47 |
| DEC opcode-selected register | — | 48–4F | 48–4F |
| INC register/memory | FE /0 | FF /0 | FF /0 |
| DEC register/memory | FE /1 | FF /1 | FF /1 |
| NOT register/memory | F6 /2 | F7 /2 | F7 /2 |
| NEG register/memory | F6 /3 | F7 /3 | F7 /3 |
| SETcc register/memory destination | 0F 90–9F | — | — |

Group `83` sign-extends its encoded byte immediate to the operand width. SETcc
writes a byte containing 0 or 1; its ModRM.reg field is ignored.
Unary forms have one destination and no immediate. The ModRM extension selects
both the operation and its fields: `F6`/`F7` /0 reads a TEST immediate, while
/2 and /3 finish after the register or address fields.

The `66` operand-size prefix selects word data; repeating it keeps that size.
Byte forms remain byte-sized with `66`. Other prefixes, including address-size
`67`, are outside the supported subset. The fifteen-byte instruction limit
includes every prefix, opcode and required operand field.

Byte register codes select AL/CL/DL/BL/AH/CH/DH/BH. Word codes select the low
sixteen bits of EAX through EDI. Byte and word writes preserve the other bits
of the parent register. Memory addresses use 32-bit ModRM/SIB base, index,
scale and displacement fields, or a 32-bit absolute offset. A0/A2 use AL;
A1/A3 use AX with `66` and EAX otherwise. Their encoded address is always four
bytes, independent of the data width.
Effective-address sums wrap at 32 bits; both frontends use flat addresses and
ignore segment bases. Blocks containing only register operands retain just the
CPU and dispatch imports.

`compile_interpreter_step()` builds a generated `step() -> i64` entry for the
same instruction subset. Both compiler functions return a `CompiledModule`
containing WebAssembly bytes and its exported entry name.

```rust
let module = wasm86_x86::compile_interpreter_step()?;
```

The step reads EIP from CPU state and fetches the instruction from paged guest
memory. A successful five-byte contiguous-range check permits direct reads of
fields within that window; later fields use checked fetch. Otherwise the exact
path checks the opcode first and reads only required fields, checking bytes in
order when a word or dword cannot be read directly. Group instructions read ModRM
and reject unsupported extensions before fetching any SIB, displacement or immediate.
For supported forms, all instruction fields are fetched before data access is checked.
No read requests byte sixteen. A wide field crossing the limit is read byte
by byte: a missing required byte below the limit faults first. If those bytes
are available, requiring byte sixteen returns general protection with error code
zero. Snapshot decoding follows the same byte order but reports truncation
or `InstructionTooLong` as construction errors. Success uses the same instruction
semantics, state publication and dispatch as snapshot blocks.

Snapshot decoding reads supplied bytes while compiling; runtime decoding reads
guest bytes during execution. Both use shared instruction forms for opcode
patterns, physical fields and operand binding. Binary forms describe the left and
right operand sources independently; either can reuse the same location resolver.
Applying the operand-size attribute fixes the data width before fields are read.
Both decoders then pass decoded operands to
shared lowering in `instruction/lower.rs`. The runtime decoder owns its byte cursor, proven
window and completion policy. Every opcode map uses the same catalog-driven
switch and selects operand decoding by encoding. Only the chosen opcode checks
its ModRM extension. Direct, checked and prefixed entries share field handling.
Memory decoding selects the SIB or ordinary layout
before reading address fields; ordinary addresses have no index term. Direct
entries retain the original fetch window across fields whose bounds fit it.
An execution builder resolves operand locations,
checks memory access, and tracks instruction progress. Binary instructions have
left and right operands; their operation determines which effects are applied.
The builder's `update` checks write permission before reading the old left value,
then runs the semantic callback and stores its result through the same checked
access. ADD, ADC, SUB, SBB, AND, OR and XOR use this operation. CMP and TEST only read
operands and set flags, so their memory operands require no write permission.
A fault publishes completed definitions into its terminating branch without
consuming the parent state used by the successful path.

The register value environment tracks typed byte, word and dword locations,
forwarding known definitions and caching reads. A location describes either a
fixed offset or a computed address together with its possible backing range;
reads and definitions use the same interface for both. State derives register
locations from the `CpuState` layout. Named CPU loads and stores use field paths,
with offsets and storage widths inferred from the Rust fields.
Reads through overlapping views synchronize earlier definitions to backing. A covering
write replaces superseded definitions. Computed register accesses synchronize
overlapping definitions, then invalidate potentially written locations. These
are completed effects; publication does not undo a partially executed instruction.

ADD, SUB and CMP retain the original operands as a lazy source for CF, PF, AF,
ZF, SF and OF. ADC adds the incoming CF; SBB subtracts it as a borrow. They read
CF after all operand guards, then construct symbolic flags from the original
operands and final result using the same arithmetic equations as ADD/SUB.
`FlagState` distinguishes stored CPU records from local flag sources.
A typed `FlagSource` retains arithmetic operands, a logical result, or explicit
result and flag values. ADC/SBB use explicit values; their incoming carry is an
input to construction and has no separate role in the retained source.
Flag-bit extraction uses typed truncation, so raw intermediates can remain
unnormalized until an operation or publication needs their logical low bit.
The `arithmetic` and `arithmetic_with_carry` constructors return this same source
type, with common result, flag and condition queries. These queries construct
expressions without a builder. Semantic operations produce a source, then call
`set_flags` once to validate and retain it as the new architectural flags.
A same-block condition uses only the expressions it needs; CMP and SUB conditions
can compare the original operands directly.

INC and DEC add or subtract one while preserving CF. They reuse arithmetic flag
equations and replace the resulting carry expression with the prior carry value,
after the operand's write checks pass. Their complete symbolic source publishes
six concrete status bytes, like ADC/SBB. NEG uses subtraction from zero and its
existing lazy record; CF is set exactly when the original operand is nonzero.
NOT inverts the operand bits and preserves the entire flag source.

At publication, state converts the current source into a `FlagRecord`: arithmetic
operands, a logical result, or six concrete status bits. Its payload variant
determines the record kind. One writer stores the payload before its kind; it does
not inspect the instruction or its incoming carry. The record exists only at this
boundary. Explicit flags are symbolic expressions; the compiler places their
evaluation where needed and leaves unused expressions unevaluated. Their array
uses logical `StatusFlag` indices; record conversion defines the CPU byte order.

ADD, SUB and CMP publish zero-extended operands to CPU dwords 4 and 8, then the
kind byte at 0. SUB kinds 1/5/9 and ADD
kinds 2/6/10 denote byte/word/dword operands. ADC/SBB sources instead publish all
six concrete flag bytes, then kind 0: the stored two-operand format cannot retain
an incoming carry. Their unused payload dwords remain untouched.
AND, OR, XOR and TEST retain only the logical result. Their records use kinds 3/7/11
with the zero-extended result at offset 4; offset 8 is unused and remains untouched.
The flag owner retains only the current source and writes its record at publication;
replacing a source does not schedule or repair individual field writes. Logic clears
CF/OF as required by x86. Its AF value is architecturally undefined; wasm86 chooses
zero. This deterministic choice avoids retaining or evaluating the old flag source.

An undefined flag still produces a normal bit when read; it does not authorize a
trap or make the guest instruction undefined behavior. An unaffected flag must
retain its previous logical value. Implementation choices for undefined flags are
made per instruction family, shared by interpreter and blocks, and documented
separately from architectural guarantees. They do not promise to reproduce a
particular physical CPU's undocumented behavior. Architecture comparisons exclude
undefined flag values; separate policy tests may assert wasm86's chosen value.
A nonzero kind owns all six status flags, so concrete flag bytes may be stale.
Kind 0 instead reads CF/PF/AF/ZF/SF/OF from bytes 12–17, each containing 0 or 1.
Other kind values trap when read. A stored direct query selects its exact record
kind before reading its typed inputs. Subtraction relations compare the original
operands; logical zero/nonzero queries compare only the result. Other queries use
shared readonly condition readers. Inverse
conditions share a reader and cached result. Readers are created only when needed;
querying a condition preserves the stored representation.
MOV and SETcc preserve flags, and these instructions leave non-status flag bytes
untouched. A faulting operand access preserves the previous instruction's flags.

The step and snapshot blocks with memory operands also import `wasm86.guest`
(minimum one Wasm page) and `wasm86.machine` (minimum 64 pages, or 4 MiB).
All three memory imports require distinct backing objects. Machine memory starts
with 2^20 little-endian 32-bit page-table entries: bit 0 marks presence, bit 1
permits writes, and bits 12–31 identify a 4-KiB frame in guest memory. Data reads
require presence. Present frames must fit the backing RAM; invalid backing is a
Wasm trap, not a guest page fault.

A missing instruction page returns the 64-bit word
`(4 << 48) | (0x10 << 32) | first_unavailable_address`. A data fault returns
`(4 << 48) | (error << 32) | first_denied_address`, where error bit 1 identifies a
write and bit 0 identifies a present but denied page. A word or dword data range
that crosses `0xffffffff` is rejected at its start with read error 0 or write error 2;
this is the current address-space policy. A one-byte access at that address fits
without consulting another page. Instruction fetch instead wraps.
All pages are checked before a data store writes any byte, including scattered
physical backing. A fault publishes earlier completed instructions and leaves EIP
at the faulting instruction; the failed instruction does not retire or dispatch.
Contiguous accesses use native loads and stores. Scattered accesses call a shared
helper for their logical width and direction, resolving each byte's physical
address after the full span passes permission checks. Memory creates each helper
when first needed and shares it across the module's functions; byte accesses need
no transfer helper.

An unsupported instruction form returns `(8 << 48) | (opcode << 32) | instruction_eip`.
The opcode field contains the first byte after any `66` prefixes, including `0F`
for an extended opcode. This reports
the implementation's unsupported subset, not an architectural invalid-opcode
fault. An instruction requiring more than fifteen bytes returns `2 << 48`, the
zero-error general-protection word, without retiring or dispatching. The step
executes one instruction and has no instruction-budget, segment or run-loop
behavior.

`wasm86-compiler` builds scalar WebAssembly functions from integer constants,
parameters and typed integer expressions. `Val<T>` represents either a standalone
literal or an expression belonging to a function body. Values such as `Val<I1>`
and `Val<I32>` carry logical integer types; function signatures use the
corresponding `Type` variants.
`Signature.result` is `Some(Type::I32)`, for example, for a returned integer, or
`None` for a function with no result. Parameters remain logical integer types.
Supported integer sizes are 1, 8, 16, 32 and 64 bits.

Create standalone values with standard Rust conversions, such as
`Val::<I32>::from(7)` or `let cleared: Val<I1> = false.into();`. Fluent operations
and typed builder operands accept `Into<Val<T>>`, so `value.add(1)` and
`body.store::<I8>(memory, 12, 9)` accept native literals directly. Signed `i32` and
unsigned `u32` literals convert to any logical integer type; `bool` converts only
to `Val<I1>`, and `u64` only to `Val<I64>`. Negative `i32` literals sign-extend to
I64, `u32` literals zero-extend, and narrower targets retain the low bits.

Standalone literals can be used in different function bodies. Once an expression
uses a body-owned value, it remains bound to that body even if it folds to a
constant. `body.value::<I32>(operand)?` admits a value into a body
and checks ownership and scope immediately. Other expression construction errors
are reported when the value is consumed. Calling `body.return_(&value)` completes
the function body; shared expressions use reusable WebAssembly locals. A raw
literal can take its logical type from the return signature, so `body.return_(7)`
is valid too.

Use `program.function(signature, |body| { ... })?` to declare and complete a
function together. The handle is returned only after the callback completes its
body. An error or missing completion discards the function and resources added
by its callback, and restores earlier forward declarations it completed. Discard
handles created by a failed callback. Separate `declare` and `define` remain
available for forward references and recursion. An open body can create a needed
helper through `body.program()`; the active function cannot be reopened.

Expressions support wrapping addition and subtraction, bitwise `and`/`or`/`xor`,
logical-width `popcnt`, `shl`, and `eq`/`ne` predicates that return `Val<I1>`. The borrowed `unsigned()` view
provides `shr`, `lt`, `ge` and zero extension, for example
`byte.unsigned().extend::<I32>().shl(8)`. `truncate::<I8>()` retains the low eight
bits. The borrowed `signed()` view provides `lt`/`ge` comparisons and sign extension, such as
`displacement.signed().extend::<I32>()`. Rust checks conversion direction;
conversions to the same type are allowed. Left shifts accept an I32 value or a
literal count; unsigned right shifts currently take literal counts.
Shift counts are modulo 32 for I1/I8/I16/I32 and modulo 64 for I64, so shifting an
I8 by 8 produces zero and shifting it by 32 preserves its value. Comparisons,
unsigned right shifts and widening read the logical low bits, including after
arithmetic that overflows a narrow type.

Build conditional code with the same builder methods:

```rust
body.if_(value.eq(0), |mut branch| {
    branch.store::<I32>(memory, 12, 9)?;
    branch.return_(7)
})?;
body.return_(value.add(1))?;
```

A branch can load, store, contain nested `if_` calls, return, tail-call or trap. Ending
its closure with `Ok(())` without a terminal lets it fall through. A false
condition skips the branch. `body.if_else(condition, then_arm, else_arm)` builds
two ordinary branches; each can fall through or terminate the function.
Child reads, call results, joined values and expressions depending on them cannot
be consumed outside that child; pure
expressions from parent values remain usable. Reads preserve snapshots across
conditional stores, which can require capturing a read before the condition.

Use `condition.select(when_true, when_false)` for a pure value choice. Both
alternatives are eager inputs, so select does not guard a load or call. Shared
inputs and the selection itself follow normal value placement. Both alternatives
must satisfy ownership and scope rules, including an unused alternative. Two literals can
specify their type with `condition.select::<I32>(7, 9)`.

Use `switch` to execute one case selected by an integer value. The callback receives
`Some(key)` for a listed case and `None` for the default. Case keys must be unique
and fit the selector's logical type. Each case can fall through, return, tail-call
or trap; falling through continues after the switch.

```rust
body.switch(&opcode, &[0x28, 0x85], |arm, key| match key {
    Some(0x28) => arm.return_(1),
    Some(0x85) => arm.return_(2),
    _ => arm.return_(0),
})?;
```

`switch_value::<I32, _>` instead returns a value. Every case, including the default,
must yield, return, tail-call or trap, and at least one must yield. Child values obey
the same scope rules as conditional arms. Dense case ranges use a Wasm branch
table; sparse ranges use balanced comparisons without allocating a large table.
Calculations using only parameters and constants can be captured separately in
each arm that uses them. Nested branches within one arm share its capture. Values
used after the join retain their common capture. Memory reads, call results, joined
values and expressions depending on them keep their snapshot rules.

Use `if_value` to execute only the selected branch and obtain its value:

```rust
let branch_value = body.if_value::<I32>(value.eq(0),
    |arm| arm.yield_(7),
    |arm| arm.yield_(value.add(1)),
)?;
body.return_(branch_value.add(2))?;
```

`yield_` consumes the direct value-arm builder and supplies the conditional's
result; `return_` still returns from the whole function. Every arm must yield,
return, tail-call or trap, and at least one arm must yield. Arm effects remain ordered,
while an unused yielded expression and its possible traps can disappear. A join
preserves raw intermediate bits; narrow values are normalized when an operation
or function boundary needs their logical low bits. Construction errors discard
both arms and leave the parent usable.

`body.trap()` consumes its builder and ends that execution path with a WebAssembly
trap, regardless of the function result type.

Functions can also finish with `body.tail_call(target, &[value.argument()])`.
The target may be imported or defined. Use `Val::argument()` or `.into()` to
combine values and literals in one `Argument` list; the call checks them against
its runtime signature. A `Val<T>` retains its logical type in this list, including
a standalone literal: `Val::<I8>::from(7)` still requires an I8 parameter. A raw
primitive such as `7.into()` instead takes its logical type from the corresponding
parameter. Returns follow the same distinction.
`Program::import_function` takes a `FunctionImport` containing the module name,
field name and logical `Signature`. Unused function imports are omitted; a direct
export also retains an imported function.

For a call that returns to the current function, use
`body.call::<I32>(helper, &[value.argument(), 7.into()])?`. Its typed result can
be shared by later expressions. A result created inside a branch stays within
that branch and its descendants. The compiler conservatively infers which
memory bytes defined helpers may read or write. Helpers without inferred writes
or unknown effects may be deferred or omitted when unused, including their
arguments and possible traps. Calls that may write, call imports or reach
unresolved recursion execute in authored order even when unused.

For a function with no result, use `body.call_void(target, arguments)?` and finish
its definition with `body.return_void()`. The same inferred effects determine
whether the invocation must execute; calls without writes or unknown effects are
omitted, including their arguments and possible traps. They have no value to discard:

```rust
let writer = program.function(Signature {
    parameters: vec![Type::I32],
    result: None,
}, |mut body| {
    let value = body.parameter::<I32>(0)?;
    body.store(memory, 12, value)?;
    body.return_void()
})?;
// In another function:
body.call_void(writer, &[7.into()])?;
```

Typed calls require a result of the requested logical type; no-result calls
require a signature with `result: None`. Returns obey the containing function's
signature, and a tail call requires matching optional result types.

WebAssembly carries 1-, 8- and 16-bit integers in `i32`. Narrow arguments must have
their unused upper bits clear, and returned narrow values satisfy the same rule.
A one-bit argument or result is therefore `0` or `1`. The compiler normalizes
narrow call arguments once per shared value without masking intermediate
arithmetic. Imported functions must return narrow results with their unused
upper bits clear too, including when the import is exported directly.
When bit bounds prove that a value is already zero or one, testing it for nonzero
reuses that value as a logical bit without another Boolean calculation.
An unshared nonzero test used only by `if_`, `if_else`, `if_value` or `select`
can pass its normalized `i32` input directly to Wasm's truth test. Numeric Boolean
uses and switch keys retain their exact zero-or-one value. Narrow inputs still
observe their logical width, and `i64` tests still inspect all 64 bits.

Fixed-offset memory access supports `I8`, `I16`, `I32` and `I64`, using 1, 2, 4
and 8 bytes respectively. `body.load::<I32>(memory, offset)` reads a snapshot;
`body.store(memory, offset, &value)` writes the value at that point in the body.
Literal operands work directly: `body.store::<I32>(memory, 12, 9)`.
Stores keep their authored order, used loads preserve their value across writes,
and unused loads are omitted. Distinct memory declarations require distinct
backing memories.

For computed addresses, use `body.load_at::<I8>(memory, &address, offset)` and
`body.store_at(memory, &address, offset, &value)`, where `address` is a `Val<I32>`
or an integer literal.
The unsigned address plus the constant displacement does not wrap. To wrap an
address calculation at 32 bits, build it explicitly with `address.add(amount)`.
Reads preserve their snapshots even when their addresses depend on other reads;
unused reads and their unused address computations are omitted.

The ordinary test suite executes generated Wasm in Wasmtime directly from Rust.
Cases supply typed inputs and independently specified expectations. The compiler
test host observes returned values, traps, host calls and imported memories. The
x86 test host observes CPU state and guest memory at dispatch and return, including
fault exits and repeated calls on the same instance. Each independent case starts
with fresh memories and runtime state; suites reuse compiled modules where possible.
Node.js is not required for these checks:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

A focused set of ignored tests runs selected cases in V8/TurboFan, covering
conditions, calls, count publication, carry arithmetic and memory faults. These
tests require Node.js 24 on `PATH`:

```sh
cargo test --workspace --locked -- --ignored
```

Each component groups its integration suites into one test executable. The
internal `wasm86-test-support` crate supplies shared engine configuration, integer
carriers, outcomes and V8 process transport; it is a development dependency of
the product crates. The compiler and x86 hosts own their distinct imports and
observations. Tests are ordinary named `#[test]` functions: build a module or
machine image, execute it, and assert returned values or state. There is no engine
selection to configure when adding a case. Only the focused V8 tests use JSON
transport, with decimal strings for 64-bit integers to preserve their bits.

Compiler behavior tests use `Fixture` to keep imported memories and callbacks
together with their host configuration. Its single-function path uses the real
compiler builder, then handles exporting and compiling. An instance accepts Rust
arguments and returns Rust values; memories and recorded callbacks are available
for direct assertions. For example:

```rust
#[test]
fn byte_addition_wraps() {
    let module = Fixture::new().function(&[Type::I8], Some(Type::I8), |body| {
        let value = body.parameter::<I8>(0)?;
        body.return_(value.add(1))
    });
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>((255,)).unwrap(), 0);
}
```

Use the fixture's `program` for multiple functions or explicit declarations, and
inspect `module.bytes()` when generated Wasm is the subject of the assertion.
Instantiation starts fresh state; further calls on that instance retain its state.

Ordinary instruction tests use `Machine`: supply code once, set named CPU
registers, and initialize virtual memory with explicit permissions. `run_step()`
and `run_block(count)` start from that initial state and expose the resulting CPU,
decoded exit and memory. CPU comparisons retain every byte, including untouched
fields; diagnostics also show register names. Tests of physical page mappings,
fault ordering or publication layout can use the lower-level image and boundary
observations directly.

Both fixtures use the public `CpuState`, so expected CPU state is an ordinary
copy with the changed fields assigned directly:

```rust
let mut machine = Machine::new(&[0x89, 0xda]); // MOV EDX, EBX
machine.cpu.registers.ebx = 42;
let mut expected = machine.state();
expected.cpu.registers.edx = 42;
expected.cpu.eip = 0x1002;
expected.cpu.instruction_count = 0; // The fixture starts at u32::MAX.
let actual = machine.run_step();
assert_eq!(actual.state, expected);
assert_eq!(actual.exit, Exit::Dispatch(0x1002));
assert_eq!(actual.dispatches, [(0x1002, expected)]);
assert!(actual.machine_unchanged);
```

For instruction sequences, update one expected CPU value and append each `Step`
immediately after its changes. Keep its expected exit and memory effects together;
a faulting instruction retains the current CPU value.

Add a named test in the relevant `tests/suites` module, or beside the component
when it needs private APIs. Cargo filters select that test directly, for example
`cargo test -p wasm86-compiler test_name`. Keep related boundary values together
when they check one behavior; give distinct scenarios their own test functions.

Execution cases check behavior against literal or independently derived results;
agreement between the two x86 frontends alone is insufficient because they share
lowering. Separate ABI and code-generation tests protect external layouts,
signatures and deliberate optimization properties. Keep guest performance
measurements in V8, separate from correctness tests. Compare generated bytes
before benchmarking a change; Wasmtime correctness results do not measure the
target engine's optimization decisions or execution speed.
