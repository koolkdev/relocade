//! Derives the portable Rust/Wasm value contract.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{quote, ToTokens};
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Fields, Generics, Index, Member, Type,
};

mod enums;
mod structs;

#[proc_macro_derive(Abi)]
pub fn derive_abi(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    if let Some(lifetime) = input.generics.lifetimes().next() {
        return Err(syn::Error::new_spanned(
            lifetime,
            "ABI values must be owned; lifetime parameters are unsupported",
        ));
    }
    match &input.data {
        Data::Struct(data) => structs::expand(input, &data.fields),
        Data::Enum(data) => enums::expand(input, data),
        Data::Union(data) => Err(syn::Error::new_spanned(
            data.union_token,
            "Abi supports structs and enums",
        )),
    }
}

fn fields(fields: &Fields) -> syn::Result<(Vec<Member>, Vec<&Type>)> {
    if fields.len() > 12 {
        return Err(syn::Error::new_spanned(
            fields,
            "ABI aggregates support up to twelve fields",
        ));
    }
    Ok((
        fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                field
                    .ident
                    .clone()
                    .map(Member::Named)
                    .unwrap_or_else(|| Member::Unnamed(Index::from(index)))
            })
            .collect(),
        fields.iter().map(|field| &field.ty).collect(),
    ))
}

fn construct(name: Tokens, fields: &Fields, values: impl IntoIterator<Item = Tokens>) -> Tokens {
    let values: Vec<_> = values.into_iter().collect();
    match fields {
        Fields::Named(named) => {
            let names = named.named.iter().map(|field| &field.ident);
            quote!(#name { #(#names: #values,)* })
        }
        Fields::Unnamed(_) => quote!(#name(#(#values,)*)),
        Fields::Unit => name,
    }
}

fn type_generics(input: &DeriveInput, types: &[&Type]) -> Generics {
    let mut generics = input.generics.clone();
    for parameter in generics.type_params_mut() {
        parameter.bounds.push(parse_quote!('static));
    }
    for ty in types {
        if depends_on_generics(ty.to_token_stream(), &generics) {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: ::relocade_wasm_bindings::AbiType));
        }
    }
    generics
}

fn depends_on_generics(tokens: Tokens, generics: &Generics) -> bool {
    tokens.into_iter().any(|token| match token {
        proc_macro2::TokenTree::Group(group) => depends_on_generics(group.stream(), generics),
        proc_macro2::TokenTree::Ident(ident) => {
            generics.params.iter().any(|parameter| match parameter {
                syn::GenericParam::Type(parameter) => parameter.ident == ident,
                syn::GenericParam::Const(parameter) => parameter.ident == ident,
                syn::GenericParam::Lifetime(_) => false,
            })
        }
        _ => false,
    })
}

fn logical(types: &[&Type]) -> Tokens {
    quote!((#(<#types as ::relocade_wasm_bindings::AbiType>::Logical,)*))
}
