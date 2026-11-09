use crate::input::construct;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DeriveInput, Ident, Path};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "WorkerResource requires a struct",
        ));
    };
    let mut worker: Option<Path> = None;
    for attribute in &input.attrs {
        if attribute.path().is_ident("worker") {
            attribute.parse_nested_meta(|meta| {
                if !meta.path.is_ident("crate") {
                    return Err(meta.error("expected #[worker(crate = path)] on a struct"));
                }
                if worker.is_some() {
                    return Err(meta.error("worker crate path is specified more than once"));
                }
                worker = Some(meta.value()?.parse()?);
                Ok(())
            })?;
        }
    }
    let worker = worker.unwrap_or_else(|| parse_quote!(::relocade_wasm_runtime::worker));

    let mut posted = None;
    for (index, field) in data.fields.iter().enumerate() {
        for attribute in &field.attrs {
            if attribute.path().is_ident("worker") {
                let kind: Ident = attribute.parse_args()?;
                if kind != "post" {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "expected #[worker(post)]",
                    ));
                }
                if posted.replace((index, field)).is_some() {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "WorkerResource requires exactly one posted field",
                    ));
                }
            }
        }
    }
    let (posted, posted_field) = posted.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "mark one field with #[worker(post)]")
    })?;
    let bindings: Vec<_> = (0..data.fields.len())
        .map(|index| format_ident!("__worker_field_{index}", span = Span::mixed_site()))
        .collect();
    let fields = construct(
        quote!(Self),
        &data.fields,
        bindings.iter().map(|name| quote!(#name)),
    );
    let mut generics = input.generics.clone();
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(Self: 'static));
    for (index, field) in data.fields.iter().enumerate() {
        let ty = &field.ty;
        let bound = if index == posted {
            parse_quote!(#ty: #worker::Post + 'static)
        } else {
            parse_quote!(#ty: ::core::marker::Send + 'static)
        };
        generics.make_where_clause().predicates.push(bound);
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let name = &input.ident;
    let posted_type = &posted_field.ty;
    let posted_binding = &bindings[posted];
    Ok(quote! {
        impl #impl_generics #worker::WorkerResource
            for #name #type_generics #where_clause
        {
            type Posted = #posted_type;

            fn prepare(self) -> #worker::ResourceTransfer<
                Self::Posted, impl ::core::ops::FnOnce(Self::Posted) -> Self + ::core::marker::Send + 'static,
            > {
                let #fields = self;
                #worker::ResourceTransfer::new(
                    #posted_binding, move |#posted_binding| #fields,
                )
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posted_field_must_be_unambiguous() {
        for input in [
            parse_quote!(
                struct Missing {
                    memory: Memory,
                }
            ),
            parse_quote!(
                struct Duplicate(#[worker(post, post)] Memory);
            ),
            parse_quote!(
                struct Multiple(#[worker(post)] Memory, #[worker(post)] Memory);
            ),
            parse_quote!(
                #[worker(post)]
                struct Misplaced(Memory);
            ),
            parse_quote!(
                struct Unknown(#[worker(send)] Memory);
            ),
            parse_quote!(
                #[worker(crate = relocade_workers, crate = another)]
                struct DuplicatePath(#[worker(post)] Memory);
            ),
            parse_quote!(
                enum Choice {
                    One(Memory),
                }
            ),
        ] {
            assert!(expand(&input).is_err());
        }
    }
}
