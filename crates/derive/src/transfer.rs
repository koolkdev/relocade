use crate::input::construct;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DeriveInput, Path};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(input, "Transfer requires a struct"));
    };
    let mut worker: Option<Path> = None;
    for attribute in &input.attrs {
        if attribute.path().is_ident("transfer") {
            attribute.parse_nested_meta(|meta| {
                if !meta.path.is_ident("crate") {
                    return Err(meta.error("expected #[transfer(crate = path)] on a struct"));
                }
                if worker.is_some() {
                    return Err(meta.error("transfer crate path is specified more than once"));
                }
                worker = Some(meta.value()?.parse()?);
                Ok(())
            })?;
        }
    }
    let worker = worker.unwrap_or_else(|| parse_quote!(::relocade_workers));
    let mut generics = input.generics.clone();
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(Self: 'static));
    for field in &data.fields {
        if let Some(attribute) = field.attrs.iter().find(|a| a.path().is_ident("transfer")) {
            return Err(syn::Error::new_spanned(
                attribute,
                "transfer behavior is defined by the field's type",
            ));
        }
        let ty = &field.ty;
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: #worker::Transfer));
    }
    let types: Vec<_> = data.fields.iter().map(|field| &field.ty).collect();
    let names = |role| {
        (0..types.len())
            .map(|index| format_ident!("__transfer_{role}_{index}", span = Span::mixed_site()))
            .collect::<Vec<_>>()
    };
    let fields = names("field");
    let states = names("state");
    let payloads = names("payload");
    let unpack = construct(
        quote!(Self),
        &data.fields,
        fields.iter().map(|v| quote!(#v)),
    );
    let rebuild = construct(
        quote!(Self),
        &data.fields,
        types.iter().zip(&states).zip(&payloads).map(
            |((ty, state), payload)| quote!(<#ty as #worker::Transfer>::decode(#state, #payload)),
        ),
    );
    let (payload_type, payload) = types
        .iter()
        .zip(&payloads)
        .map(|(ty, name)| (quote!(<#ty as #worker::Transfer>::Payload), quote!(#name)))
        .reduce(|(left_type, left), (right_type, right)| {
            (
                quote!(#worker::PayloadPair<#left_type, #right_type>),
                quote!(#worker::PayloadPair(#left, #right)),
            )
        })
        .unwrap_or_else(|| (quote!(()), quote!(())));
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #worker::Transfer for #name #type_generics #where_clause {
            type State = (#(<#types as #worker::Transfer>::State,)*);
            type Payload = #payload_type;

            fn encode(self) -> (Self::State, Self::Payload) {
                let #unpack = self;
                #(let (#states, #payloads) = <#types as #worker::Transfer>::encode(#fields);)*
                ((#(#states,)*), #payload)
            }

            fn decode(state: Self::State, payload: Self::Payload) -> Self {
                let (#(#states,)*) = state;
                let #payload = payload;
                #rebuild
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_policies_and_invalid_containers_are_rejected() {
        for input in [
            parse_quote!(
                struct Field(#[transfer(resource)] u32);
            ),
            parse_quote!(
                #[transfer(resource)]
                struct Container(u32);
            ),
            parse_quote!(
                #[transfer(crate = relocade_workers, crate = another)]
                struct Duplicate(u32);
            ),
            parse_quote!(
                enum Choice {
                    One(u32),
                }
            ),
            parse_quote!(union Overlay { one: u32, two: f32 }),
        ] {
            assert!(expand(&input).is_err());
        }
    }
}
