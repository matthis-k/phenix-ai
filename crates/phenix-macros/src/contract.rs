use crate::{args, no_extra, required};
use quote::quote;
use syn::{
    spanned::Spanned, FnArg, GenericArgument, ItemTrait, PathArguments, ReturnType, TraitItem, Type,
};

pub fn expand(
    attr: proc_macro2::TokenStream,
    item: ItemTrait,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut attributes = args(attr)?;
    let id = required(&mut attributes, "id")?;
    no_extra(&attributes)?;
    if !phenix_contract::operation_key(&format!("{id}/probe")) {
        return Err(syn::Error::new(item.span(), "invalid contract identity"));
    }
    if !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
        || !item.supertraits.is_empty()
        || item.unsafety.is_some()
        || item.auto_token.is_some()
    {
        return Err(syn::Error::new(
            item.span(),
            "contract traits must be nongeneric declarations",
        ));
    }
    let revision = id
        .rsplit_once('@')
        .unwrap()
        .1
        .parse::<u64>()
        .map_err(|_| syn::Error::new(item.span(), "contract revision must fit u64"))?;
    let mut methods = Vec::new();
    for member in &item.items {
        let TraitItem::Fn(method) = member else {
            return Err(syn::Error::new(
                member.span(),
                "contracts contain only operation methods",
            ));
        };
        let signature = &method.sig;
        if method.default.is_some()
            || signature.asyncness.is_some()
            || signature.constness.is_some()
            || signature.unsafety.is_some()
            || signature.abi.is_some()
            || signature.variadic.is_some()
            || !signature.generics.params.is_empty()
            || signature.generics.where_clause.is_some()
            || method
                .attrs
                .iter()
                .any(|attribute| !attribute.path().is_ident("doc"))
        {
            return Err(syn::Error::new(
                method.span(),
                "operation must be an unconditional synchronous declaration",
            ));
        }
        let mut inputs = signature.inputs.iter();
        let Some(FnArg::Receiver(receiver)) = inputs.next() else {
            return Err(syn::Error::new(
                signature.span(),
                "operation needs a borrowed receiver",
            ));
        };
        if receiver.reference.is_none() || receiver.colon_token.is_some() {
            return Err(syn::Error::new(
                receiver.span(),
                "operation needs &self or &mut self",
            ));
        }
        let request = match inputs.next() {
            None => quote!(()),
            Some(FnArg::Typed(argument)) => {
                let ty = &argument.ty;
                quote!(#ty)
            }
            Some(argument) => return Err(syn::Error::new(argument.span(), "invalid request")),
        };
        if inputs.next().is_some() {
            return Err(syn::Error::new(
                signature.span(),
                "use one portable request struct",
            ));
        }
        let ReturnType::Type(_, result) = &signature.output else {
            return Err(syn::Error::new(
                signature.span(),
                "operation returns Result<Response, Error>",
            ));
        };
        let Type::Path(path) = result.as_ref() else {
            return Err(syn::Error::new(
                result.span(),
                "operation returns Result<Response, Error>",
            ));
        };
        let names: Vec<_> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if path.qself.is_some()
            || !matches!(
                names.join("::").as_str(),
                "Result" | "std::result::Result" | "core::result::Result"
            )
            || path
                .path
                .segments
                .iter()
                .rev()
                .skip(1)
                .any(|segment| !matches!(segment.arguments, PathArguments::None))
        {
            return Err(syn::Error::new(path.span(), "use the standard Result type"));
        }
        let PathArguments::AngleBracketed(arguments) =
            &path.path.segments.last().unwrap().arguments
        else {
            return Err(syn::Error::new(
                path.span(),
                "Result needs response and error types",
            ));
        };
        let types: Vec<_> = arguments.args.iter().collect();
        let [GenericArgument::Type(response), GenericArgument::Type(error)] = types.as_slice()
        else {
            return Err(syn::Error::new(
                arguments.span(),
                "Result needs response and error types",
            ));
        };
        let symbol = &signature.ident;
        let name = symbol.to_string().trim_start_matches("r#").to_owned();
        let key = format!("{id}/{name}");
        if !phenix_contract::operation_key(&key) {
            return Err(syn::Error::new(symbol.span(), "invalid operation identity"));
        }
        methods.push((name.clone(), quote! {
            #[allow(non_camel_case_types)]
            pub struct #symbol;
            impl ::phenix_sdk::live::Operation for #symbol {
                type Request = #request;
                type Response = #response;
                type Error = #error;
                const KEY: &'static str = #key;
            }
            impl #symbol {
                pub const DEFINITION: ::phenix_sdk::StaticOperation = ::phenix_sdk::StaticOperation {
                    name: #name,
                    key: #key,
                    request: <#request as ::phenix_sdk::live::Portable>::SCHEMA,
                    response: <#response as ::phenix_sdk::live::Portable>::SCHEMA,
                    error: <#error as ::phenix_sdk::live::Portable>::SCHEMA,
                };
            }
        }, quote!(#symbol::DEFINITION)));
    }
    if methods.is_empty() {
        return Err(syn::Error::new(item.span(), "contract needs an operation"));
    }
    methods.sort_by(|a, b| a.0.cmp(&b.0));
    if methods.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(syn::Error::new(item.span(), "duplicate operation"));
    }
    let declarations = methods.iter().map(|method| &method.1);
    let operations = methods.iter().map(|method| &method.2);
    let name = &item.ident;
    let visibility = &item.vis;
    let attributes = &item.attrs;
    Ok(quote! {
        #(#attributes)*
        #[allow(non_snake_case)]
        #visibility mod #name {
            #[allow(unused_imports)]
            use super::*;
            #(#declarations)*
            pub const CONTRACT: ::phenix_sdk::StaticContract = ::phenix_sdk::StaticContract {
                contract: #id,
                revision: #revision,
                operations: &[#(#operations),*],
            };
        }
    })
}

#[cfg(test)]
mod tests {
    use super::expand;
    use quote::quote;

    #[test]
    fn rejects_unsupported_contract_declarations() {
        for source in [
            "trait Bad {}",
            "trait Bad { type Item; }",
            "trait Bad: Send { fn call(&self) -> Result<String, String>; }",
            "trait Bad { fn call(self) -> Result<String, String>; }",
            "trait Bad { fn call() -> Result<String, String>; }",
            "trait Bad { fn call<T>(&self, input: T) -> Result<String, String>; }",
            "trait Bad { #[cfg(any())] fn call(&self) -> Result<String, String>; }",
            "trait Bad { fn call(&self) -> other::Result<String, String>; }",
        ] {
            let item = syn::parse_str(source).unwrap();
            assert!(
                expand(quote!(id = "example.bad@1"), item).is_err(),
                "{source}"
            );
        }
        for attributes in [
            quote!(id = "invalid"),
            quote!(id = "example.bad@0"),
            quote!(id = "example.bad@1", unknown = "value"),
            quote!(id = "example.bad@1", id = "example.bad@1"),
        ] {
            let item =
                syn::parse_str("trait Good { fn call(&self) -> Result<String, String>; }").unwrap();
            assert!(expand(attributes, item).is_err());
        }
    }
}
