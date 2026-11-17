//! Derives and adapters for ABI values, host services and worker transfers.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

mod compiler_value;
mod input;
mod interrupt;
mod interrupt_dispatch;
mod transfer;
mod wasm_abi;
mod win32_args;
mod win32_export;

/// Binds an ordinary Rust service to a Microsoft x86 DLL export.
#[proc_macro_attribute]
pub fn win32_export(args: TokenStream, item: TokenStream) -> TokenStream {
    win32_export::expand(args.into(), parse_macro_input!(item as syn::ItemFn))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives the guest stack layout of a Microsoft x86 parameter record.
#[proc_macro_derive(Win32Args)]
pub fn derive_win32_args(input: TokenStream) -> TokenStream {
    win32_args::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Transfer, attributes(transfer))]
pub fn derive_transfer(input: TokenStream) -> TokenStream {
    transfer::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn interrupt(args: TokenStream, item: TokenStream) -> TokenStream {
    let binding = parse_macro_input!(args as interrupt::Binding);
    let function = parse_macro_input!(item as syn::ItemFn);
    interrupt::expand(binding, function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro]
pub fn interrupt_handler(input: TokenStream) -> TokenStream {
    let target = parse_macro_input!(input as syn::Expr);
    interrupt::call::handler(target)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro]
pub fn interrupt_dispatch(input: TokenStream) -> TokenStream {
    let selection = parse_macro_input!(input as interrupt_dispatch::Selection);
    selection
        .expand()
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(WasmAbi)]
pub fn derive_wasm_abi(input: TokenStream) -> TokenStream {
    derive(input, wasm_abi::structs::expand, wasm_abi::enums::expand)
}

#[proc_macro_derive(CompilerValue)]
pub fn derive_compiler_value(input: TokenStream) -> TokenStream {
    derive(
        input,
        compiler_value::structs::expand,
        compiler_value::enums::expand,
    )
}

fn derive(
    input: TokenStream,
    structs: fn(&DeriveInput, &Fields) -> syn::Result<Tokens>,
    enums: fn(&DeriveInput, &syn::DataEnum) -> syn::Result<Tokens>,
) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input, structs, enums)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(
    input: &DeriveInput,
    structs: fn(&DeriveInput, &Fields) -> syn::Result<Tokens>,
    enums: fn(&DeriveInput, &syn::DataEnum) -> syn::Result<Tokens>,
) -> syn::Result<Tokens> {
    input::validate(input)?;
    match &input.data {
        Data::Struct(data) => structs(input, &data.fields),
        Data::Enum(data) => enums(input, data),
        Data::Union(data) => Err(syn::Error::new_spanned(
            data.union_token,
            "ABI derives support structs and enums",
        )),
    }
}
