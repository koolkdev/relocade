//! Derives the portable Rust/Wasm value contract.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

mod input;
mod wasm_abi;

#[proc_macro_derive(WasmAbi)]
pub fn derive_wasm_abi(input: TokenStream) -> TokenStream {
    derive(input, wasm_abi::structs::expand, wasm_abi::enums::expand)
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
