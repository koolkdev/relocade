use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::Fields;

pub(super) mod enums;
pub(super) mod structs;

fn compiler_fields(fields: &Fields) -> Tokens {
    match fields {
        Fields::Named(named) => {
            let fields = named.named.iter().map(|field| {
                let (vis, name, ty) = (&field.vis, &field.ident, &field.ty);
                quote!(#vis #name: <#ty as ::relocade_compiler::abi::CompilerValue>::Values)
            });
            quote!({ #(#fields,)* })
        }
        Fields::Unnamed(unnamed) => {
            let fields = unnamed.unnamed.iter().map(|field| {
                let (vis, ty) = (&field.vis, &field.ty);
                quote!(#vis <#ty as ::relocade_compiler::abi::CompilerValue>::Values)
            });
            quote!((#(#fields,)*))
        }
        Fields::Unit => quote!(),
    }
}
