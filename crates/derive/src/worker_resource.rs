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
    let mut transfers = Vec::new();
    for (field, binding) in data.fields.iter().zip(&bindings) {
        let mut resource = false;
        for attribute in &field.attrs {
            if attribute.path().is_ident("worker") {
                let value: Ident = attribute.parse_args()?;
                if value != "resource" {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "expected #[worker(resource)]",
                    ));
                }
                if std::mem::replace(&mut resource, true) {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "a field can have only one worker attribute",
                    ));
                }
            }
        }
        let ty = &field.ty;
        if resource {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #worker::WorkerResource));
            transfers.push((
                quote!(<#ty as #worker::WorkerResource>::Posted),
                quote!(#worker::WorkerResource::prepare(#binding)),
                quote!(#binding),
            ));
        } else {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: ::core::marker::Send + 'static));
        }
    }
    let (posted_type, transfer, received) = transfers
        .into_iter()
        .reduce(
            |(posted, prepare, received), (next_posted, next_prepare, next_received)| {
                (
                    quote!((#posted, #next_posted)),
                    quote!(#prepare.zip(#next_prepare)),
                    quote!((#received, #next_received)),
                )
            },
        )
        .ok_or_else(|| {
            syn::Error::new_spanned(
                &input.ident,
                "mark at least one field with #[worker(resource)]",
            )
        })?;
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#posted_type: #worker::Post + 'static));
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let name = &input.ident;
    Ok(quote! {
        impl #impl_generics #worker::WorkerResource
            for #name #type_generics #where_clause
        {
            type Posted = #posted_type;

            fn prepare(self) -> #worker::ResourceTransfer<
                Self::Posted, impl ::core::ops::FnOnce(Self::Posted) -> Self + ::core::marker::Send + 'static,
            > {
                let #fields = self;
                #transfer.map(move |#received| #fields)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_attributes_must_be_unambiguous() {
        for input in [
            parse_quote!(
                struct Missing {
                    memory: Memory,
                }
            ),
            parse_quote!(
                struct Duplicate(#[worker(resource, resource)] Memory);
            ),
            parse_quote!(
                struct Conflict(
                    #[worker(resource)]
                    #[worker(resource)]
                    Memory,
                );
            ),
            parse_quote!(
                #[worker(resource)]
                struct Misplaced(Memory);
            ),
            parse_quote!(
                struct Unknown(#[worker(send)] Memory);
            ),
            parse_quote!(
                struct Raw(#[worker(post)] Memory);
            ),
            parse_quote!(
                #[worker(crate = relocade_workers, crate = another)]
                struct DuplicatePath(#[worker(resource)] Memory);
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
