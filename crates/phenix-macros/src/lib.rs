use proc_macro::TokenStream;
mod contract;
mod definitions;
mod property;
mod state;
use quote::quote;
use syn::{
    parse_macro_input, spanned::Spanned, Data, DeriveInput, Expr, Fields, ItemMod, ItemType, Meta,
};
fn args(
    tokens: proc_macro2::TokenStream,
) -> syn::Result<std::collections::BTreeMap<String, String>> {
    use syn::parse::Parser;
    let metas =
        syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated.parse2(tokens)?;
    let mut result = std::collections::BTreeMap::new();
    for m in metas {
        let Meta::NameValue(n) = m else {
            return Err(syn::Error::new(m.span(), "expected name = string"));
        };
        let Expr::Lit(v) = n.value else {
            return Err(syn::Error::new(n.span(), "expected string"));
        };
        let syn::Lit::Str(s) = v.lit else {
            return Err(syn::Error::new(v.span(), "expected string"));
        };
        let key = n
            .path
            .get_ident()
            .ok_or_else(|| syn::Error::new(n.path.span(), "expected key"))?
            .to_string();
        if result.insert(key, s.value()).is_some() {
            return Err(syn::Error::new(n.path.span(), "duplicate attribute"));
        }
    }
    Ok(result)
}
#[proc_macro_attribute]
pub fn contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::ItemTrait);
    contract::expand(attr.into(), item)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
#[proc_macro_attribute]
pub fn property(attr: TokenStream, item: TokenStream) -> TokenStream {
    property::expand(attr.into(), parse_macro_input!(item as ItemType))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
fn required(
    map: &mut std::collections::BTreeMap<String, String>,
    key: &str,
) -> syn::Result<String> {
    map.remove(key)
        .ok_or_else(|| syn::Error::new(proc_macro2::Span::call_site(), format!("missing {key}")))
}
fn no_extra(map: &std::collections::BTreeMap<String, String>) -> syn::Result<()> {
    if map.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "unknown annotation field",
        ))
    }
}
#[proc_macro_attribute]
pub fn plugin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let module = parse_macro_input!(item as ItemMod);
    let attr: proc_macro2::TokenStream = attr.into();
    let result = match definitions::selected(attr.clone()) {
        Ok(true) => definitions::expand(attr, module),
        Ok(false) => state::expand(attr, module),
        Err(error) => Err(error),
    };
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Derive the canonical portable contract representation.
#[proc_macro_derive(Portable)]
pub fn portable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = (|| {
        if !input.generics.params.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "live portable generics require an explicit implementation",
            ));
        }
        let name = &input.ident;
        let (schema, to, from) = match &input.data {
            Data::Struct(s) => {
                let Fields::Named(fields) = &s.fields else {
                    return Err(syn::Error::new(
                        s.fields.span(),
                        "use named live portable fields",
                    ));
                };
                let mut fields: Vec<_> = fields.named.iter().collect();
                fields.sort_by_key(|f| f.ident.as_ref().unwrap().to_string());
                let names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
                let keys: Vec<_> = names.iter().map(|n| n.to_string()).collect();
                let types: Vec<_> = fields.iter().map(|f| &f.ty).collect();
                (
                    quote!(::phenix_sdk::StaticSchema::Struct(&[#(::phenix_sdk::StaticField{name:#keys,schema:<#types as ::phenix_sdk::live::Portable>::SCHEMA}),*])),
                    quote!(::phenix_sdk::Value::map([#((#keys,::phenix_sdk::live::Portable::to_value(&self.#names))),*])),
                    quote! {let ::phenix_sdk::Value::Map(mut fields)=value else{return Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"expected live struct"));};let result=Self{#(#names:<#types as ::phenix_sdk::live::Portable>::from_value(fields.remove(#keys).ok_or_else(||::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,concat!("missing field ",#keys)))?)?),*};if !fields.is_empty(){return Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"unknown live field"));}Ok(result)},
                )
            }
            Data::Enum(e) => {
                if e.variants.iter().any(|v| !matches!(v.fields, Fields::Unit)) {
                    return Err(syn::Error::new(
                        e.enum_token.span,
                        "live portable variants must be unit variants",
                    ));
                }
                let mut names: Vec<_> = e.variants.iter().map(|v| &v.ident).collect();
                names.sort_by_key(|v| v.to_string());
                let keys: Vec<_> = names.iter().map(|v| v.to_string()).collect();
                (
                    quote!(::phenix_sdk::StaticSchema::Variants(&[#(#keys),*])),
                    quote!(::phenix_sdk::Value::String(match self{#(Self::#names=>#keys),*}.into())),
                    quote! {let ::phenix_sdk::Value::String(name)=value else{return Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"expected live variant"));};match name.as_str(){#(#keys=>Ok(Self::#names),)*_=>Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"unknown live variant"))}},
                )
            }
            _ => {
                return Err(syn::Error::new(
                    input.span(),
                    "unsupported live portable type",
                ))
            }
        };
        Ok(
            quote! {impl ::phenix_sdk::live::Portable for #name{const SCHEMA: ::phenix_sdk::StaticSchema=#schema;fn to_value(&self)->::phenix_sdk::Value{#to}fn from_value(value: ::phenix_sdk::Value)->Result<Self,::phenix_sdk::Failure>{#from}}},
        )
    })();
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}
