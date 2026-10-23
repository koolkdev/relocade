//! Shared structural validation and field handling for both derives.

use proc_macro2::TokenStream as Tokens;
use quote::{quote, ToTokens};
use syn::{parse_quote, Data, DeriveInput, Fields, Generics, Index, Member, Path, Type};

pub(super) fn validate(input: &DeriveInput) -> syn::Result<()> {
    if let Some(lifetime) = input.generics.lifetimes().next() {
        return Err(syn::Error::new_spanned(
            lifetime,
            "ABI values must be owned; lifetime parameters are unsupported",
        ));
    }
    if let Data::Enum(data) = &input.data {
        if !input.generics.params.is_empty() {
            return Err(syn::Error::new_spanned(
                &input.generics,
                "ABI enums must be concrete",
            ));
        }
        if data.variants.is_empty() {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "ABI enums require at least one variant",
            ));
        }
    }
    Ok(())
}

pub(super) fn fields(fields: &Fields) -> syn::Result<(Vec<Member>, Vec<&Type>)> {
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

pub(super) fn construct(
    name: Tokens,
    fields: &Fields,
    values: impl IntoIterator<Item = Tokens>,
) -> Tokens {
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

pub(super) fn type_generics(input: &DeriveInput, types: &[&Type], bound: Path) -> Generics {
    let mut generics = input.generics.clone();
    for parameter in generics.type_params_mut() {
        parameter.bounds.push(parse_quote!('static));
    }
    for ty in types {
        if depends_on_generics(ty.to_token_stream(), &generics) {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #bound));
        }
    }
    generics
}

pub(super) fn depends_on_generics(tokens: Tokens, generics: &Generics) -> bool {
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

pub(super) fn logical(types: &[&Type]) -> Tokens {
    quote!((#(<#types as ::relocade_wasm_bindings::AbiType>::Logical,)*))
}
