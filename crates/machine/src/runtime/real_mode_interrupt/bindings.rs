//! Declare an ABI on the implementation and select services in a separate table.
//!
//! `#[interrupt]` preserves the function's Rust API and generates an internal async
//! `name_entry`. `interrupt_dispatch!` selects those entries by register value without
//! repeating their argument or result mappings. Synchronous services finish in
//! one poll; asynchronous services use their ordinary Rust awaits.
//!
//! ```
//! use relocade_machine::MemoryView;
//! use relocade_machine::real_mode_interrupt::{interrupt, interrupt_dispatch};
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
//! interrupt_dispatch! {
//!     video(ah) {
//!         0x03: position,
//!         _: unsupported,
//!     }
//! }
//! // video_entry(call).await routes AH=03h to the generated position_entry.
//! # fn main() {}
//! ```
//!
//! Context inputs named `memory`, `cpu`, and `io` borrow the corresponding
//! [`crate::CpuContext`] fields. `context(call)` explicitly supplies the whole
//! context to a handler that needs it. Other parameters remain explicit; methods
//! retain their receiver. A table can route to `keyboard.read_key`, and can name
//! typed parameters after its selector: `keyboard(ah, keyboard: &Keyboard)`.
//!
//! Register arguments are captured before borrowing service resources or awaiting.
//! Scalar names use the existing CPU accessors; `(cx, dx)` represents a u32 with
//! CX as its high word. Records map each field to a register or word pair, for
//! example `args(point = Point { x: cx, y: dx })`. Computation belongs in the
//! ordinary Rust function, rather than in an argument mapping.
//!
//! Return arms write only their declared locations on normal completion. `cf` and
//! `zf` edit saved interrupt FLAGS; omitted registers and flags are preserved.
//! A result arm such as `None => unsupported` delegates to that annotated service,
//! using its generated adapter just like a table entry, including the `_` fallback.
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

pub use relocade_derive::{interrupt, interrupt_dispatch};
