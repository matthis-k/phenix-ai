use crate::{args, no_extra, required};
use quote::quote;
use syn::{parse::Parser, spanned::Spanned, Expr, ItemMod, Meta};

fn declarations(tokens: proc_macro2::TokenStream) -> syn::Result<Vec<Meta>> {
    Ok(
        syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated
            .parse2(tokens)?
            .into_iter()
            .collect(),
    )
}

pub fn selected(tokens: proc_macro2::TokenStream) -> syn::Result<bool> {
    for meta in declarations(tokens)? {
        let Meta::NameValue(value) = meta else {
            continue;
        };
        if !value.path.is_ident("profile") {
            continue;
        }
        let Expr::Lit(value) = value.value else {
            continue;
        };
        let syn::Lit::Str(text) = value.lit else {
            continue;
        };
        if text.value() == "definitions" {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn expand(
    tokens: proc_macro2::TokenStream,
    mut module: ItemMod,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut attributes = std::collections::BTreeMap::new();
    let mut contracts = Vec::new();
    for meta in declarations(tokens)? {
        if let Meta::List(list) = &meta {
            if !list.path.is_ident("define") {
                return Err(syn::Error::new(
                    list.span(),
                    "definition-only plugins use define declarations",
                ));
            }
            let contract: syn::Path = syn::parse2(list.tokens.clone())?;
            contracts.push(quote!(#contract::CONTRACT));
        } else {
            for (key, value) in args(quote!(#meta))? {
                if attributes.insert(key, value).is_some() {
                    return Err(syn::Error::new(meta.span(), "duplicate plugin attribute"));
                }
            }
        }
    }
    let id = required(&mut attributes, "id")?;
    let version = required(&mut attributes, "version")?;
    let profile = required(&mut attributes, "profile")?;
    no_extra(&attributes)?;
    if profile != "definitions"
        || id == "root"
        || !phenix_contract::identifier(&id)
        || !phenix_contract::identifier(&version)
    {
        return Err(syn::Error::new(
            module.span(),
            "invalid definition plugin identity or profile",
        ));
    }
    if contracts.is_empty() {
        return Err(syn::Error::new(
            module.span(),
            "definition plugin needs a contract",
        ));
    }
    let (_, items) = module.content.as_mut().ok_or_else(|| {
        syn::Error::new(
            module.ident.span(),
            "definition plugin needs an inline module",
        )
    })?;
    if items
        .iter()
        .any(|item| matches!(item, syn::Item::Struct(state) if state.ident == "State"))
    {
        return Err(syn::Error::new(
            module.ident.span(),
            "definition plugin has no native State",
        ));
    }
    items.push(syn::parse2(quote! {
        pub const METADATA: ::phenix_sdk::MetadataBytes =
            ::phenix_sdk::descriptor::static_metadata::definitions(#id, #version, [#(#contracts),*]);
    })?);
    Ok(quote!(#module))
}
