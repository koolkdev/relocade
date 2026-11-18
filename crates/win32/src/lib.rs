//! Microsoft x86 function bindings for hosted PE32 applications.
//!
//! A signature describes both a guest call to a built-in export and a host call
//! to a guest callback. Addresses are flat 32-bit virtual addresses. The embedding
//! owns the address space, dispatch and lifetime of suspended calls.
//!
//! The `#[win32_export]` attribute preserves an ordinary Rust function and
//! defines its parameter record and [`Export`] binding in the type namespace.
//! Exports default to `stdcall`. Use `abi = "cdecl"` for caller cleanup and
//! `name = "..."` to select the DLL symbol. Both sync and async handlers can
//! return an integer, a guest pointer, `()`, or a `Result` carrying [`Exception`].
//! Integer and pointer results use EAX or EDX:EAX. Fixed floating-point
//! parameters use their four- or eight-byte guest stack representations.
//!
//! ```
//! use relocade_win32::{win32_export, Exception, Export, GuestMemory, GuestPtr};
//!
//! #[win32_export(name = "ReadCounter")]
//! fn read_counter(#[memory] memory: &dyn GuestMemory, address: GuestPtr<u32>)
//!     -> Result<u32, Exception>
//! {
//!     let mut bytes = [0; 4];
//!     memory.read(address.address(), &mut bytes)?;
//!     Ok(u32::from_le_bytes(bytes))
//! }
//! assert_eq!(read_counter::NAME, "ReadCounter");
//! assert_eq!(read_counter::SIGNATURE.stub(), [0x90, 0x90, 0xc2, 4, 0]);
//! ```
//!
//! Mark an application resource with `#[context]` on a shared reference and
//! pass its owner to [`Exports::bind`]. A cdecl service can receive a trailing
//! `#[varargs] tail: VaList` and read promoted arguments through guest memory.
//! The guest's import slot and dynamically obtained function pointer can both
//! point to the same guest address once [`Exports::install`] publishes the hooks.
//! [`DllLoader::load`] installs all functions declared by a [`BuiltinDll`].
//! A PE image loader gets the module once per import descriptor, resolves
//! each symbol with [`LoadedDll::resolve`], and writes the importing image's
//! address table. Dynamic lookups use the same guest addresses. Guest code
//! that walks the PE headers also needs a mapped export directory.
//!
//! ```compile_fail
//! use relocade_win32::{win32_export, VaList};
//! #[win32_export]
//! fn bad_cleanup(#[varargs] tail: VaList) {}
//! ```
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

#[cfg(test)]
extern crate self as relocade_win32;

mod dll;
mod export;
mod exports;
mod loader;
mod signature;
mod stack;
mod value;

pub use dll::{BuiltinDll, DllExport, DllSymbol};
pub use export::{Export, ExportResult};
pub use exports::Exports;
pub use loader::{DllLoadError, DllLoader, LoadedDll};
pub use relocade_derive::{win32_export, Win32Args};
pub use relocade_x86::Exception;
pub use signature::{Callback, CallingConvention, Signature};
pub use stack::{Arguments, GuestMemory, StackReader, VaList};
pub use value::{Argument, GuestPtr, Promoted, ReturnRegisters, ReturnValue};
