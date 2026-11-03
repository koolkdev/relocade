//! Invoke a service through its generated register adapter.

use proc_macro2::{Ident, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{punctuated::Punctuated, Expr, Member};

pub(crate) fn entry(target: Expr, call: &Ident) -> syn::Result<Tokens> {
    let (target, args) = match target {
        Expr::Call(call) => (*call.func, call.args),
        Expr::MethodCall(mut method) => {
            method.method = format_ident!("{}_entry", method.method);
            method.args.insert(0, syn::parse_quote!(&mut *#call));
            return Ok(quote!(#method));
        }
        target => (target, Punctuated::new()),
    };
    match target {
        Expr::Path(mut path) => {
            let function = &mut path.path.segments.last_mut().unwrap().ident;
            *function = format_ident!("{function}_entry");
            let path = &path.path;
            let qualifier =
                (path.segments.len() == 1 && path.leading_colon.is_none()).then(|| quote!(self::));
            Ok(quote!(#qualifier #path(&mut *#call, #args)))
        }
        Expr::Field(field) => {
            let receiver = field.base;
            let Member::Named(name) = field.member else {
                return Err(syn::Error::new_spanned(
                    field.member,
                    "expected a method name",
                ));
            };
            let name = format_ident!("{name}_entry");
            Ok(quote!(#receiver.#name(&mut *#call, #args)))
        }
        target => Err(syn::Error::new_spanned(
            target,
            "expected a service function or receiver.method",
        )),
    }
}
