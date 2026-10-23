use super::compiler_fields;
use crate::input::{construct, fields, type_generics};
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{parse_quote, DeriveInput, Fields, Index};

pub(crate) fn expand(input: &DeriveInput, definition: &Fields) -> syn::Result<Tokens> {
    let name = &input.ident;
    let values_name = format_ident!("{name}Values");
    let vis = &input.vis;
    let (members, types) = fields(definition)?;
    let indices: Vec<_> = (0..types.len()).map(Index::from).collect();
    let generics = type_generics(
        input,
        &types,
        parse_quote!(::relocade_compiler::abi::CompilerValue),
    );
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let symbolic = compiler_fields(definition);
    let declaration = match definition {
        Fields::Named(_) => quote!(#vis struct #values_name #generics #where_clause #symbolic),
        _ => quote!(#vis struct #values_name #generics #symbolic #where_clause;),
    };
    let from_values = construct(
        quote!(#values_name),
        definition,
        types.iter().zip(&indices).map(|(ty, index)| {
            quote!(<#ty as ::relocade_compiler::abi::CompilerValue>::from_values(values.#index))
        }),
    );
    let clone_values = construct(
        quote!(Self),
        definition,
        members.iter().map(|member| quote!(self.#member.clone())),
    );
    let into_values = if types.is_empty() {
        Tokens::new()
    } else {
        quote!((#(<#types as ::relocade_compiler::abi::CompilerValue>::into_values(values.#members),)*))
    };
    Ok(quote! {
        #[doc = "Named symbolic fields for generated code."]
        #declaration
        impl #impl_generics ::core::clone::Clone for #values_name #ty_generics #where_clause {
            fn clone(&self) -> Self { #clone_values }
        }
        impl #impl_generics ::relocade_compiler::abi::CompilerValue for #name #ty_generics #where_clause {
            type Values = #values_name #ty_generics;
            fn from_values(values: <Self::Logical as ::relocade_compiler::Results>::Values) -> Self::Values { #from_values }
            fn into_values(values: Self::Values) -> <Self::Logical as ::relocade_compiler::Results>::Values {
                #into_values
            }
        }
        impl #impl_generics ::core::convert::From<#values_name #ty_generics> for ::relocade_compiler::Arguments #where_clause {
            fn from(values: #values_name #ty_generics) -> Self { (#(values.#members,)*).into() }
        }
    })
}
