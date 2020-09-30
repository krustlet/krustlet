//! A crate for deriving state machine traits in Kubelet. Right now this crate only consists of a
//! derive macro for the `TransitionTo` trait. In addition to the `derive` attribute, this macro
//! also requires the use of a custom attribute called `transition_to` that specifies the types that
//! can be transitioned to. Not specifying this attribute will result in a compile time error. A
//! simple example of this is below:
//!
//! ```rust,no_run
//! use kubelet_derive::TransitionTo;
//!
//! pub struct VolumeMount;
//! pub struct ImagePullBackoff;
//!
//! #[derive(Default, Debug, TransitionTo)]
//! #[transition_to(VolumeMount, ImagePullBackoff)]
//! pub struct ImagePull;
//!```

extern crate proc_macro;

use crate::proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Error, Meta};

const ATTRIBUTE_NAME: &str = "transition_to";

#[proc_macro_derive(TransitionTo, attributes(transition_to))]
pub fn derive_transition_to(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let mut token_stream = TokenStream::new();

    // We need to check if we found at least one attribute. If it isn't set, we need to error
    let mut found_attr = false;
    for attr in input.attrs.into_iter() {
        if let Meta::List(parsed_attr) = attr.parse_meta().unwrap() {
            if let Some(id) = parsed_attr.path.get_ident() {
                if id == ATTRIBUTE_NAME {
                    found_attr = true;
                    for transition_type in parsed_attr.nested.iter() {
                        let expanded = quote! {
                            impl #impl_generics kubelet::state::TransitionTo<#transition_type> for #name #ty_generics #where_clause {}
                        };
                        token_stream.extend(TokenStream::from(expanded));
                    }
                }
            }
        }
    }

    if !found_attr {
        let message = format!(
            "No `{}` attribute found for `{}`. Please specify at least one type to transition to",
            ATTRIBUTE_NAME,
            name.to_string()
        );
        TokenStream::from(Error::new(name.span(), message).to_compile_error())
    } else {
        token_stream
    }
}
