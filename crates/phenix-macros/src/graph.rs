//! Const-friendly declarations for event and action discovery.
//! The generated constants use the public SDK; no native ABI code is emitted.
use crate::{args, no_extra, required};
use quote::quote;
use syn::{spanned::Spanned, ItemStruct};

pub fn event(
    attr: proc_macro2::TokenStream,
    item: ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut args = args(attr)?;
    let id = required(&mut args, "id")?;
    no_extra(&args)?;
    if !phenix_contract::identifier(&id) {
        return Err(syn::Error::new(item.span(), "invalid event identity"));
    }
    let name = &item.ident;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new(item.span(), "event types must be concrete"));
    }
    Ok(quote! {
        #item
        impl ::phenix_sdk::GraphEventPayload for #name {
            const ID: &'static str = #id;
        }
        impl #name {
            pub const EVENT: ::phenix_sdk::StaticEventDefinition =
                ::phenix_sdk::StaticEventDefinition {
                    id: #id,
                    payload: <Self as ::phenix_sdk::live::Portable>::SCHEMA,
                };
        }
    })
}

pub fn action(
    attr: proc_macro2::TokenStream,
    item: ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut args = args(attr)?;
    let id = required(&mut args, "id")?;
    let event = required(&mut args, "event")?;
    let description = required(&mut args, "description")?;
    let required = args.remove("requires").unwrap_or_default();
    no_extra(&args)?;
    if !phenix_contract::identifier(&id)
        || !phenix_contract::identifier(&event)
        || description.trim().is_empty()
        || description.len() > 4096
        || !item.generics.params.is_empty()
    {
        return Err(syn::Error::new(item.span(), "invalid action declaration"));
    }
    let mut operations: Vec<_> = if required.trim().is_empty() {
        vec![]
    } else {
        required.split(',').map(str::trim).collect()
    };
    if operations
        .iter()
        .any(|key| !phenix_contract::operation_key(key))
    {
        return Err(syn::Error::new(item.span(), "invalid required operation"));
    }
    operations.sort_unstable();
    if operations.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(syn::Error::new(item.span(), "duplicate required operation"));
    }
    let name = &item.ident;
    Ok(quote! {
        #item
        impl #name {
            pub const ACTION: ::phenix_sdk::StaticActionDefinition =
                ::phenix_sdk::StaticActionDefinition {
                    id: #id,
                    event: #event,
                    description: #description,
                    requires: &[#(#operations),*],
                };
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_events_and_actions_reject_at_expansion() {
        assert!(event(
            quote!(id = "invalid/name"),
            syn::parse_quote!(
                struct Bad;
            )
        )
        .is_err());
        assert!(action(
            quote!(
                id = "a",
                event = "e",
                description = "Do",
                requires = "invalid"
            ),
            syn::parse_quote!(
                struct Bad;
            )
        )
        .is_err());
        assert!(action(
            quote!(
                id = "a",
                event = "e",
                description = "Do",
                requires = "test.a@1/x,test.a@1/x"
            ),
            syn::parse_quote!(
                struct Bad;
            )
        )
        .is_err());
        assert!(event(
            quote!(id = "good.event"),
            syn::parse_quote!(
                struct Good;
            )
        )
        .is_ok());
    }
}
