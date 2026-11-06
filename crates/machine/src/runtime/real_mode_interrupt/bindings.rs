//! Declare register ABIs on ordinary Rust functions and select services with match.
//!
//! `#[interrupt]` preserves the function's Rust API and generates an internal async
//! ABI adapter. `interrupt_dispatch!` selects those adapters within a Rust match
//! expression without repeating argument or result mappings. Dispatchers use the
//! same `#[interrupt]` attribute to bind their selector and context.
//! `interrupt_handler!` supplies an adapter to the existing registration API.
//! Synchronous services finish in one poll; asynchronous services use their
//! ordinary Rust awaits.
//!
//! ```
//! use relocade_machine::{CpuContext, FarPointer, MemoryView};
//! use relocade_machine::real_mode_interrupt::{
//!     interrupt, interrupt_dispatch, interrupt_handler, Interrupts,
//! };
//!
//! #[interrupt(context(memory), args(page = bh), returns {
//!     Some(position) => { (cx, dx) = position, zf = false },
//!     None => { zf = true },
//! })]
//! fn position(memory: &MemoryView<'_>, page: u8) -> Option<u32> {
//!     (page == 0).then(|| memory.read_u32(0x400).unwrap())
//! }
//!
//! #[interrupt(returns { status => { ah = status, cf = true } })]
//! fn unsupported() -> u8 { 0x86 }
//! #[interrupt(context(call), args(function = ah))]
//! async fn video(call: &mut CpuContext<'_>, function: u8) {
//!     interrupt_dispatch!(call, match function {
//!         0x03 => position,
//!         _ => unsupported,
//!     });
//! }
//! fn register(interrupts: &mut Interrupts<'_>, entry: FarPointer) {
//!     interrupts.register_interrupt(0x10, entry, interrupt_handler!(video));
//! }
//! # fn main() {}
//! ```
//!
//! Non-exhaustive matches are rejected by Rust:
//! ```compile_fail
//! use relocade_machine::{CpuContext, real_mode_interrupt::{interrupt, interrupt_dispatch}};
//! #[interrupt]
//! fn service() {}
//! async fn incomplete(call: &mut CpuContext<'_>, function: u8) {
//!     interrupt_dispatch!(call, match function { 0 => service });
//! }
//! ```
//!
//! Context inputs named `memory`, `cpu`, and `io` borrow the corresponding
//! [`crate::CpuContext`] fields. `context(call)` explicitly supplies the whole
//! context to a handler that needs it. Other parameters remain explicit; methods
//! retain their receiver. Match arms can route to `keyboard.read_key` or pass
//! explicit arguments, as in `read_character(input)`. Methods also accept explicit
//! arguments.
//! The first argument to `interrupt_dispatch!` names the mutable context. Its second
//! argument is a Rust match expression whose arm bodies name services. Patterns,
//! guards and exhaustiveness follow Rust's rules; no wildcard arm is required
//! when the other patterns cover every value. Each selected adapter is awaited.
//! The context remains available in the scrutinee and guards. Arm and let-guard
//! bindings must not shadow its name, so an arm cannot invoke another context.
//! `interrupt_handler!(video)` returns the adapter function, retaining any explicit
//! parameters for callers that supply resources per invocation. To bind resources
//! at registration, use `interrupt_handler!(keyboard::dispatch(&keyboard))` or
//! `interrupt_handler!(keyboard.read_key)`. These forms produce an async move
//! closure that owns its captures and evaluates its arguments on each invocation.
//! Generic services retain their bounds and `where` clauses. Type and const
//! arguments can be supplied explicitly, as in `service::<_, 3>(&resource)`;
//! the adapter forwards them to the service while Rust infers lifetimes.
//!
//! Register arguments are captured before borrowing service resources or awaiting.
//! Scalar names use the existing CPU accessors; `(cx, dx)` represents a u32 with
//! CX as its high word. Records map each field to a register or word pair, for
//! example `args(point = Point { x: cx, y: dx })`. Computation belongs in the
//! ordinary Rust function, rather than in an argument mapping.
//! Segment names such as `args(segment = es)` capture the visible u16 selector.
//! Returning `ds = segment` also loads its real-mode segment cache, just as a
//! pointer result does. Both forms share segment alias and continuation checks.
//!
//! `ptr(es, dx)` captures a [`super::FarPointer`]: the visible segment value and
//! a 16-bit offset. It also works in record fields. Services access memory through
//! [`crate::MemoryView`]; the mapping does not copy buffers or create Rust slices.
//! Buffer length, format and offset wrapping remain the service's responsibility.
//! These bindings use the machine's ordinary real-mode contract: segment bases
//! equal their selector shifted by four. Protected-mode pointers need a different ABI.
//!
//! A pointer result takes a [`super::FarPointer`] and loads the destination's
//! real-mode segment cache as well as its offset. ES, DS, FS and GS can be written;
//! CS and SS must remain intact for the interrupt continuation. For example:
//! ```
//! use relocade_machine::real_mode_interrupt::{interrupt, FarPointer};
//! #[interrupt(args(source = ptr(es, dx)), returns {
//!     address => { ptr(es, bp) = address },
//! })]
//! fn address(source: FarPointer) -> FarPointer { source }
//! # fn main() {}
//! ```
//!
//! Return arms write only their declared locations on normal completion. `cf` and
//! `zf` edit saved interrupt FLAGS; omitted registers and flags are preserved.
//! A result arm such as `None => unsupported` delegates to that annotated service,
//! using its generated adapter just like a match arm.
//! It can forward result values, for example `Err(error) => failure(error)`.
//! Imported services use their module path so the generated adapter is reachable.
//! The existing hook owns suspension, IRQ continuation and IRET.
//! A function returning `()` can omit `returns`; its adapter writes no outputs.
//! Other return types require an explicit mapping:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::interrupt;
//! #[interrupt(args(value = ax))]
//! fn unmapped_result(value: u16) -> u16 { value }
//! # fn main() {}
//! ```
//!
//! Register alias checks use the same declarations as CPU accessors:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::interrupt;
//! #[interrupt(returns { value => { ax = value, al = 0u8 } })]
//! fn overlapping() -> u16 { 0 }
//! # fn main() {}
//! ```
//! The same rejection applies to methods, even if their entries are never called:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::interrupt;
//! struct Service;
//! impl Service {
//!     #[interrupt(returns { value => { ax = value, al = 0u8 } })]
//!     fn overlapping(&self) -> u16 { 0 }
//! }
//! # fn main() {}
//! ```
//! Widths are checked by Rust without implicit truncation:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::interrupt;
//! #[interrupt(returns { value => { al = value } })]
//! fn wrong_width() -> u16 { 256 }
//! # fn main() {}
//! ```
//! High/low pairs must contain distinct words:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::interrupt;
//! #[interrupt(returns { value => { (cx, cx) = value } })]
//! fn repeated_word() -> u32 { 0 }
//! # fn main() {}
//! ```
//!
//! Pointer outputs participate in the same alias checks, including the segment:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::{interrupt, FarPointer};
//! #[interrupt(returns { p => { ptr(es, bp) = p, ptr(es, di) = p } })]
//! fn conflicting_segments(p: FarPointer) -> FarPointer { p }
//! # fn main() {}
//! ```
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::{interrupt, FarPointer};
//! #[interrupt(returns { p => { ptr(es, bp) = p, es = 0u16 } })]
//! fn overlapping_selector(p: FarPointer) -> FarPointer { p }
//! # fn main() {}
//! ```
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::{interrupt, FarPointer};
//! #[interrupt(returns { p => { ptr(es, bp) = p, ebp = 0u32 } })]
//! fn conflicting_offsets(p: FarPointer) -> FarPointer { p }
//! # fn main() {}
//! ```
//! Pointer offsets must be 16-bit registers:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::{interrupt, FarPointer};
//! #[interrupt(args(source = ptr(es, edx)))]
//! fn wide_offset(source: FarPointer) {}
//! # fn main() {}
//! ```
//! Continuation segments cannot be returned:
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::{interrupt, FarPointer};
//! #[interrupt(returns { p => { ptr(cs, bp) = p } })]
//! fn overwrite_code(p: FarPointer) -> FarPointer { p }
//! # fn main() {}
//! ```
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::{interrupt, FarPointer};
//! #[interrupt(returns { p => { ptr(ss, bp) = p } })]
//! fn overwrite_stack(p: FarPointer) -> FarPointer { p }
//! # fn main() {}
//! ```
//! ```compile_fail
//! use relocade_machine::real_mode_interrupt::interrupt;
//! #[interrupt(returns { selector => { cs = selector } })]
//! fn overwrite_code(selector: u16) -> u16 { selector }
//! # fn main() {}
//! ```

pub use relocade_derive::{interrupt, interrupt_dispatch, interrupt_handler};
