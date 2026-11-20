//! Microsoft x86 function bindings for hosted PE32 applications.
//!
//! A signature describes both a guest call to a built-in export and a host call
//! to a guest callback. Addresses are flat 32-bit virtual addresses. The embedding
//! owns the address space, dispatch and lifetime of suspended calls.
//!
//! Define callback arguments in declaration order. Each field occupies a guest
//! stack slot, independently of the Rust record's layout.
//!
//! ```
//! use relocade_win32::{CallingConvention, Signature, Win32Args};
//!
//! #[derive(Win32Args)]
//! struct WindowMessage { window: u32, message: u32, wparam: u32, lparam: i32 }
//! const WNDPROC: Signature<WindowMessage, i32> = Signature::new(CallingConvention::Stdcall);
//! assert_eq!(WNDPROC.stub(), [0x90, 0x90, 0xc2, 16, 0]);
//! ```
//!
//! Guest addresses have an explicit 32-bit representation.
//!
//! ```compile_fail
//! use relocade_win32::Win32Args;
//! #[derive(Win32Args)]
//! struct WrongWidth { address: usize }
//! ```
//!
//! See the [Microsoft x86 calling conventions](https://learn.microsoft.com/en-us/cpp/cpp/argument-passing-and-naming-conventions).

#![forbid(unsafe_code)]

mod signature;
mod stack;
mod value;

pub use relocade_derive::Win32Args;
pub use relocade_x86::Exception;
pub use signature::{Callback, CallingConvention, Signature};
pub use stack::{Arguments, GuestMemory, StackReader, VaList};
pub use value::{Argument, GuestPtr, Promoted, ReturnRegisters, ReturnValue};
