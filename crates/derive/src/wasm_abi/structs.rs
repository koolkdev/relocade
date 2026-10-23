use crate::input::{construct, depends_on_generics, fields, logical, type_generics};
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{parse_quote, DeriveInput, Fields, Index};

pub(crate) fn expand(input: &DeriveInput, definition: &Fields) -> syn::Result<Tokens> {
    let name = &input.ident;
    let (members, types) = fields(definition)?;
    let indices: Vec<_> = (0..types.len()).map(Index::from).collect();
    let generics = type_generics(
        input,
        &types,
        parse_quote!(::relocade_wasm_bindings::AbiType),
    );
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let tuple = quote!((#(#types,)*));
    let logical = logical(&types);
    let mut value_generics = generics.clone();
    if depends_on_generics(tuple.clone(), &generics) {
        value_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(
                #tuple: ::relocade_wasm_bindings::AbiValue<Value = #tuple, Logical = #logical>
            ));
        value_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(
                #logical: ::relocade_wasm_bindings::WasmRepr
            ));
    }
    let (value_impl, _, value_where) = value_generics.split_for_impl();
    let lift = construct(
        quote!(Self),
        definition,
        indices.iter().map(|index| quote!(values.#index)),
    );
    Ok(quote! {
        impl #impl_generics ::relocade_wasm_bindings::AbiType for #name #ty_generics #where_clause {
            type Logical = #logical;
        }
        impl #value_impl ::relocade_wasm_bindings::AbiValue for #name #ty_generics #value_where {
            type Value = Self;

            #[inline]
            fn lower(value: Self) -> ::relocade_wasm_bindings::Carrier<Self> {
                <#tuple as ::relocade_wasm_bindings::AbiValue>::lower((#(value.#members,)*))
            }
            #[inline]
            fn lift(values: ::relocade_wasm_bindings::Carrier<Self>) -> ::core::option::Option<Self> {
                let values = <#tuple as ::relocade_wasm_bindings::AbiValue>::lift(values)?;
                ::core::option::Option::Some(#lift)
            }
        }
    })
}
