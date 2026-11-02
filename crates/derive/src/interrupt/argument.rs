//! Input mappings are registers, word pairs, pointers, or records of those fields.

use super::register::Register;
use proc_macro2::{Ident, TokenStream as Tokens};
use quote::quote;
use syn::Expr;

pub(super) fn read(value: &Expr, cpu: &Ident) -> syn::Result<Tokens> {
    if let Expr::Struct(record) = value {
        if record.rest.is_some() || record.qself.is_some() {
            return Err(syn::Error::new_spanned(
                record,
                "expected an explicit record of register fields",
            ));
        }
        let path = &record.path;
        let fields = record
            .fields
            .iter()
            .map(|field| {
                let member = &field.member;
                let attrs = &field.attrs;
                let value = read_register(&field.expr, cpu)?;
                Ok(quote!(#(#attrs)* #member: #value))
            })
            .collect::<syn::Result<Vec<_>>>()?;
        Ok(quote!(#path { #(#fields),* }))
    } else {
        read_register(value, cpu)
    }
}

fn read_register(value: &Expr, cpu: &Ident) -> syn::Result<Tokens> {
    syn::parse2::<Register>(quote!(#value))
        .map_err(|_| {
            syn::Error::new_spanned(
                value,
                "expected a register, word pair, or ptr(segment, offset)",
            )
        })?
        .read(cpu)
}
