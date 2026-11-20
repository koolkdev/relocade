use crate::input::construct;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Data, DeriveInput};

pub(super) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Win32Args requires a parameter record",
        ));
    };
    let name = &input.ident;
    let members = data.fields.members();
    let types: Vec<_> = data.fields.iter().map(|field| &field.ty).collect();
    let mut generics = input.generics.clone();
    for ty in &types {
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: ::relocade_win32::Argument));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let record = construct(
        quote!(Self),
        &data.fields,
        types.iter().map(|ty| quote!(stack.read::<#ty>()?)),
    );
    Ok(quote! {
        impl #impl_generics ::relocade_win32::Arguments for #name #ty_generics #where_clause {
            const STACK_BYTES: u32 = 0 #( + <#types as ::relocade_win32::Argument>::STACK_BYTES )*;
            fn read(stack: &mut ::relocade_win32::StackReader<'_>)
                -> ::core::result::Result<Self, ::relocade_win32::Exception>
            {
                Ok(#record)
            }
            fn encode(&self, bytes: &mut ::std::vec::Vec<u8>) {
                #(bytes.extend_from_slice(::relocade_win32::Argument::encode(&self.#members).as_ref());)*
            }
        }
    })
}
