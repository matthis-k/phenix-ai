use crate::{args, no_extra, required};
use quote::quote;
use syn::{parse::Parser, spanned::Spanned, ItemType, Meta};

pub fn expand(
    tokens: proc_macro2::TokenStream,
    alias: ItemType,
) -> syn::Result<proc_macro2::TokenStream> {
    if !alias.generics.params.is_empty() || alias.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            alias.span(),
            "property aliases are nongeneric",
        ));
    }
    let metas =
        syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated.parse2(tokens)?;
    let mut attributes = std::collections::BTreeMap::new();
    let mut access = [false; 3];
    for meta in metas {
        if let Meta::Path(path) = &meta {
            let Some(index) = ["get", "set", "watch"]
                .iter()
                .position(|name| path.is_ident(name))
            else {
                return Err(syn::Error::new(meta.span(), "unknown property operation"));
            };
            if access[index] {
                return Err(syn::Error::new(meta.span(), "duplicate property operation"));
            }
            access[index] = true;
        } else {
            for (key, value) in args(quote!(#meta))? {
                if attributes.insert(key, value).is_some() {
                    return Err(syn::Error::new(meta.span(), "duplicate property attribute"));
                }
            }
        }
    }
    let owner = required(&mut attributes, "owner")?;
    let id = required(&mut attributes, "id")?;
    no_extra(&attributes)?;
    if owner == "root" || !phenix_contract::identifier(&owner) || !phenix_contract::identifier(&id)
    {
        return Err(syn::Error::new(alias.span(), "invalid property identity"));
    }
    symbols(&owner, &id, &alias.ident, &alias.vis, &alias.ty, access)
}

pub fn symbols(
    owner: &str,
    id: &str,
    name: &syn::Ident,
    vis: &syn::Visibility,
    ty: &syn::Type,
    access: [bool; 3],
) -> syn::Result<proc_macro2::TokenStream> {
    let contract = format!("{owner}.property.{id}@1");
    if !phenix_contract::operation_key(&format!("{contract}/probe")) || !access.iter().any(|v| *v) {
        return Err(syn::Error::new(
            name.span(),
            "invalid property contract or missing operation",
        ));
    }
    let mut methods = Vec::new();
    if access[0] {
        methods.push(quote!(fn get(&self, location: ::phenix_sdk::live::ValueRef<#ty>) -> Result<#ty, ::phenix_sdk::live::PropertyError>;));
    }
    if access[1] {
        methods.push(quote!(fn set(&mut self, update: ::phenix_sdk::live::PropertyUpdate<#ty>) -> Result<u64, ::phenix_sdk::live::PropertyError>;));
    }
    if access[2] {
        methods.push(quote!(fn watch(&self, location: ::phenix_sdk::live::ValueRef<#ty>) -> Result<u64, ::phenix_sdk::live::PropertyError>;));
    }
    let declaration = syn::parse2(quote!(#vis trait #name { #(#methods)* }))?;
    let mut module: syn::ItemMod = syn::parse2(crate::contract::expand(
        quote!(id = #contract),
        declaration,
    )?)?;
    module
        .content
        .as_mut()
        .unwrap()
        .1
        .push(syn::parse2(quote!(pub const ID: &str = #id;))?);
    for (operation, enabled) in ["get", "set", "watch"].into_iter().zip(access) {
        if enabled {
            let symbol = syn::Ident::new(operation, name.span());
            module.content.as_mut().unwrap().1.push(syn::parse2(quote! {
                impl #symbol {
                    pub fn locate(host: &::phenix_sdk::live::Host<'_>, slot: &str)
                        -> Result<::phenix_sdk::live::ValueRef<#ty>, ::phenix_sdk::Failure>
                    {
                        host.locate_property(slot, <Self as ::phenix_sdk::Operation>::KEY)
                    }
                }
            })?);
        }
    }
    Ok(quote!(#module))
}
