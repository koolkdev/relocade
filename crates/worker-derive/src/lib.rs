//! Derives field composition for `relocade-worker` messages.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{parse_macro_input, parse_quote, Data, DeriveInput, Fields, Generics};

#[proc_macro_derive(Message)]
pub fn derive_message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let mut generics = input.generics.clone();
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(Self: 'static));
    let (encode, decode) = match &input.data {
        Data::Struct(data) => {
            let Record {
                pattern,
                encode,
                decode,
            } = record(quote!(Self), &data.fields, &mut generics);
            (quote! { let #pattern = self; #encode }, decode)
        }
        Data::Enum(data) => {
            let mut encode_arms = Vec::new();
            let mut decode_arms = Vec::new();
            for (tag, variant) in data.variants.iter().enumerate() {
                let name = &variant.ident;
                let Record {
                    pattern,
                    encode,
                    decode,
                } = record(quote!(Self::#name), &variant.fields, &mut generics);
                let tag = tag as u32;
                encode_arms.push(quote! {
                    #pattern => ::relocade_worker::web::__private::encode_fields([
                        ::relocade_worker::Message::encode(#tag), #encode,
                    ])
                });
                decode_arms.push(quote! { #tag => { #decode } });
            }
            (
                quote! { match self { #(#encode_arms),* } },
                quote! {
                    let [__relocade_message_tag, __relocade_message_value] =
                        ::relocade_worker::web::__private::decode_fields(__relocade_message_value)?;
                    match <::core::primitive::u32 as ::relocade_worker::Message>::decode(__relocade_message_tag)? {
                        #(#decode_arms,)*
                        _ => ::core::result::Result::Err(::relocade_worker::Error::Failed(
                            "invalid worker message variant".into(),
                        )),
                    }
                },
            )
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "Message supports structs and enums",
            ))
        }
    };
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        #[cfg(target_arch = "wasm32")]
        impl #impl_generics ::relocade_worker::Message for #name #type_generics #where_clause {
            fn encode(self) -> ::relocade_worker::web::Packet { #encode }

            fn decode(__relocade_message_value: ::relocade_worker::web::Value) -> ::relocade_worker::Result<Self> {
                #decode
            }
        }
    })
}

struct Record {
    pattern: Tokens,
    encode: Tokens,
    decode: Tokens,
}

fn record(constructor: Tokens, fields: &Fields, generics: &mut Generics) -> Record {
    let bindings: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let ty = &field.ty;
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: ::relocade_worker::Message));
            format_ident!("__relocade_field_{index}", span = Span::mixed_site())
        })
        .collect();
    let construct = |values: Vec<Tokens>| match fields {
        Fields::Named(fields) => {
            let names = fields.named.iter().map(|field| &field.ident);
            quote! { #constructor { #(#names: #values),* } }
        }
        Fields::Unnamed(_) => quote! { #constructor(#(#values),*) },
        Fields::Unit => constructor.clone(),
    };
    let pattern = construct(bindings.iter().map(|binding| quote!(#binding)).collect());
    let decoded = construct(
        bindings
            .iter()
            .map(|binding| quote!(::relocade_worker::Message::decode(#binding)?))
            .collect(),
    );
    Record {
        pattern,
        encode: quote! {
            ::relocade_worker::web::__private::encode_fields([
                #(::relocade_worker::Message::encode(#bindings)),*
            ])
        },
        decode: quote! {
            let [#(#bindings),*] =
                ::relocade_worker::web::__private::decode_fields(__relocade_message_value)?;
            ::core::result::Result::Ok(#decoded)
        },
    }
}
