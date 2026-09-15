#[path = "plugin_attr_core.rs"]
mod core;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeSet;
use syn::{
    parse::Parser, parse_quote, punctuated::Punctuated, Attribute, Expr, ExprLit, Fields, Ident,
    ImplItem, Item, ItemImpl, ItemStruct, Lit, LitStr, Meta, Token, Type,
};

const ROOT_FIELD: &str = "__phenix_root";

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    if let Ok(item) = syn::parse2::<ItemImpl>(input.clone()) {
        validate_lifecycle_runtime_abi(&item)?;
        if has_root_behavior(&item)? {
            return expand_root_impl(args, item);
        }
        let expanded = core::expand(args, input)?;
        return append_lifecycle_runtime(expanded, &item);
    }

    let Ok(item) = syn::parse2::<ItemStruct>(input.clone()) else {
        return core::expand(args, input);
    };

    let (args, root, remaps) = split_root_arguments(args)?;
    expand_struct(args, item, root, &remaps)
}

fn expand_struct(
    args: TokenStream,
    mut item: ItemStruct,
    root_requested: bool,
    remaps: &[ExposeRemap],
) -> syn::Result<TokenStream> {
    let root = should_inject_root(&args, &item, root_requested)?;
    let name = item.ident.clone();
    let root_ident = root_component_ident(&name);
    let exposed_fields = if root {
        crate::expose_attr::plugin_exposed_fields(&mut item)?
    } else {
        Vec::new()
    };
    let exposed_fields_impl = root.then(|| {
        let fields_impl = crate::expose_attr::exposed_fields_impl(&name, &exposed_fields);
        let remaps = remaps.iter().map(|remap| {
            let from = &remap.from;
            let to = &remap.to;
            quote!(::phenix_sdk::StaticExposeRemap::new(#from, #to))
        });
        quote! {
            #fields_impl

            impl ::phenix_sdk::StaticRootExpose for #name {
                const EXPOSED_REMAPS: &'static [::phenix_sdk::StaticExposeRemap] =
                    &[#(#remaps),*];
            }
        }
    });

    let expanded = if root {
        let id = plugin_id_literal(args.clone(), &name)?;
        let mut core_item = item.clone();
        let Fields::Named(fields) = &mut core_item.fields else {
            unreachable!("root injection is restricted to named plugin structs")
        };
        let root_ty: Type = parse_quote!(#root_ident);
        let field: syn::Field = parse_quote! {
            #[phenix(component, id = #id)]
            __phenix_root: #root_ty
        };
        fields.named.push(field);
        let expanded = core::expand(args.clone(), quote!(#core_item))?;
        strip_synthetic_root(expanded, &name)?
    } else {
        core::expand(args.clone(), quote!(#item))?
    };

    let components = component_fields(&item)?;
    let root_dispatch = root.then(|| {
        quote! {
            if component == &Self::component_id() {
                return ::phenix_sdk::StaticComponentRuntimeDispatch::dispatch_runtime(
                    self,
                    service,
                    input,
                    host,
                );
            }
        }
    });
    let dispatch_arms = components.iter().map(|component| {
        let field = &component.field;
        let id = component_id(component);

        quote! {
            if component == &#id {
                return ::phenix_sdk::StaticComponentRuntimeDispatch::dispatch_runtime(
                    &self.#field,
                    service,
                    input,
                    host,
                );
            }
        }
    });
    let root_layer = root.then(|| {
        quote! {
            if let Some(result) = ::phenix_sdk::StaticComponentRuntimeDispatch::dispatch_layer_runtime(
                self,
                service,
                input,
                host,
            ) {
                return result;
            }
        }
    });
    let layer_arms = components.iter().map(|component| {
        let field = &component.field;
        quote! {
            if let Some(result) = ::phenix_sdk::StaticComponentRuntimeDispatch::dispatch_layer_runtime(
                &self.#field,
                service,
                input,
                host,
            ) {
                return result;
            }
        }
    });
    let root_listener_binding = root.then(|| {
        quote! {
            if component == &Self::component_id() {
                let listener = <#root_ident as ::phenix_sdk::StaticComponentBehavior>::listeners()
                    .into_iter()
                    .find(|listener| listener.method == method)?;
                let state = ::std::sync::Weak::clone(&state);
                let owner = owner.clone();
                let component = component.clone();
                let method = listener.method;
                return Some(::phenix_sdk::StaticPluginInstance::<Self>::listener_handler(
                    owner,
                    method,
                    move |envelope, host| {
                        let Some(state) = state.upgrade() else {
                            return Err(Box::new(::std::io::Error::other(format!(
                                "stateful listener {method} plugin state is unavailable"
                            ))) as Box<dyn ::std::error::Error + Send + Sync>);
                        };
                        let plugin = state;
                        let context = ::phenix_sdk::EventContext::from_event(
                            host,
                            component.clone(),
                            envelope,
                        );
                        ::phenix_sdk::StaticComponentRuntimeDispatch::dispatch_listener_runtime(
                            &*plugin,
                            method,
                            &context,
                            &envelope.payload,
                        )
                        .unwrap_or_else(|| Err(Box::new(::std::io::Error::other(format!(
                            "unsupported plugin listener: {method}"
                        ))) as Box<dyn ::std::error::Error + Send + Sync>))
                    },
                ));
            }
        }
    });
    let listener_bindings = components.iter().map(|component| {
        let field = &component.field;
        let ty = &component.ty;
        let id = component_id(component);
        quote! {
            if component == &#id {
                let listener = <#ty as ::phenix_sdk::StaticComponentBehavior>::listeners()
                    .into_iter()
                    .find(|listener| listener.method == method)?;
                let state = ::std::sync::Weak::clone(&state);
                let owner = owner.clone();
                let component = component.clone();
                let method = listener.method;
                return Some(::phenix_sdk::StaticPluginInstance::<Self>::listener_handler(
                    owner,
                    method,
                    move |envelope, host| {
                        let Some(state) = state.upgrade() else {
                            return Err(Box::new(::std::io::Error::other(format!(
                                "stateful listener {method} plugin state is unavailable"
                            ))) as Box<dyn ::std::error::Error + Send + Sync>);
                        };
                        let plugin = state;
                        let context = ::phenix_sdk::EventContext::from_event(
                            host,
                            component.clone(),
                            envelope,
                        );
                        ::phenix_sdk::StaticComponentRuntimeDispatch::dispatch_listener_runtime(
                            &plugin.#field,
                            method,
                            &context,
                            &envelope.payload,
                        )
                        .unwrap_or_else(|| Err(Box::new(::std::io::Error::other(format!(
                            "unsupported component listener: {method}"
                        ))) as Box<dyn ::std::error::Error + Send + Sync>))
                    },
                ));
            }
        }
    });

    Ok(quote! {
        #expanded

        #exposed_fields_impl

        impl ::phenix_sdk::StaticPluginComponentDispatch for #name {
            fn dispatch_component(
                &self,
                component: &::phenix_sdk::__phenix_plugin::ComponentId,
                service: &::phenix_sdk::__phenix_plugin::ServiceId,
                input: &[u8],
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                #root_dispatch
                #(#dispatch_arms)*
                Err(format!("unsupported static plugin component: {component}"))
            }

            fn dispatch_layer(
                &self,
                service: &::phenix_sdk::__phenix_plugin::ServiceId,
                input: &[u8],
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<::phenix_sdk::LayerResult, String> {
                #root_layer
                #(#layer_arms)*
                Err(format!("unsupported static plugin layer: {service}"))
            }

            fn listener_handler(
                state: ::std::sync::Weak<Self>,
                owner: &::phenix_sdk::__phenix_plugin::PluginId,
                component: &::phenix_sdk::__phenix_plugin::ComponentId,
                method: &str,
                _generation: &::phenix_sdk::__phenix_plugin::GraphGenerationId,
            ) -> Option<::std::sync::Arc<dyn ::phenix_sdk::__phenix_plugin::PluginListener>>
            where
                Self: Send + Sync + 'static,
            {
                #root_listener_binding
                #(#listener_bindings)*
                None
            }
        }
    })
}

fn expand_root_impl(args: TokenStream, item: ItemImpl) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "plugin behavior impls inherit plugin identity and do not accept root arguments",
        ));
    }
    if item.trait_.is_some() || !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item,
            "plugin behavior must be a non-generic inherent impl",
        ));
    }

    let self_ident = self_type_ident(&item)?;
    let root_ident = root_component_ident(&self_ident);
    let lifecycle = lifecycle_trait_impl(&item)?;
    let (behavior, markers) = normalize_root_behavior(item.clone())?;
    let expanded = crate::component_runtime_attr::expand(TokenStream::new(), quote!(#behavior))?;
    let expanded = retarget_behavior(expanded, &root_ident, &item.self_ty)?;
    let expanded = quote! {
        #expanded
        #lifecycle
        #(#markers)*

        #[doc(hidden)]
        struct #root_ident;

        impl ::phenix_sdk::StaticComponentDefinition for #root_ident {}
        impl ::phenix_sdk::StaticComponentImports for #root_ident {}
    };
    append_lifecycle_runtime(expanded, &item)
}

fn lifecycle_trait_impl(item: &ItemImpl) -> syn::Result<TokenStream> {
    let mut has_lifecycle = false;
    let mut lifecycle = item.clone();
    for member in &mut lifecycle.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        let mut retained = Vec::new();
        for attribute in std::mem::take(&mut method.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            if lifecycle_role(&attribute)?.is_some() {
                has_lifecycle = true;
                retained.push(attribute);
            }
        }
        method.attrs = retained;
    }
    if !has_lifecycle {
        return Ok(TokenStream::new());
    }
    let expanded = core::expand(TokenStream::new(), quote!(#lifecycle))?;
    let file = syn::parse2::<syn::File>(expanded)?;
    file.items
        .into_iter()
        .find_map(|item| match item {
            Item::Impl(item) if trait_is(&item, "StaticPluginLifecycle") => Some(quote!(#item)),
            _ => None,
        })
        .ok_or_else(|| syn::Error::new_spanned(item, "plugin lifecycle metadata was not generated"))
}

fn normalize_root_behavior(mut item: ItemImpl) -> syn::Result<(ItemImpl, Vec<TokenStream>)> {
    let self_ty = (*item.self_ty).clone();
    let self_ident = self_type_ident(&item)?;
    let mut markers = Vec::new();
    let mut exposed_names = BTreeSet::new();

    for member in &mut item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        let phenix = method
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("phenix"))
            .count();
        if phenix > 1 {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                "a plugin method may declare only one Phenix role",
            ));
        }

        let mut retained = Vec::new();
        for mut attribute in std::mem::take(&mut method.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            if lifecycle_role(&attribute)?.is_some() {
                continue;
            }
            if let Some(expose) = expose_declaration(&attribute, &method.sig.ident)? {
                let marker = expose_marker_ident(&self_ident, &method.sig.ident);
                let public_name = expose.name;
                if !exposed_names.insert(public_name.value()) {
                    return Err(syn::Error::new_spanned(
                        public_name,
                        "duplicate exposed method path",
                    ));
                }
                let authority = expose.authority;
                let export: Attribute = match authority {
                    Some(authority) => parse_quote! {
                        #[phenix(export(#marker), public, authority = #authority)]
                    },
                    None => parse_quote! {
                        #[phenix(export(#marker), public)]
                    },
                };
                retained.push(export);
                markers.push(quote! {
                    #[doc(hidden)]
                    #[allow(non_camel_case_types)]
                    struct #marker;

                    impl ::phenix_sdk::InterfaceMarker for #marker {
                        fn interface_id() -> ::phenix_sdk::__phenix_plugin::InterfaceId {
                            <#self_ty as ::phenix_sdk::StaticRootExpose>::root_exposed_interface(
                                #public_name,
                            )
                        }
                    }
                });
                continue;
            }
            normalize_on_event(&mut attribute)?;
            retained.push(attribute);
        }
        method.attrs = retained;
    }

    Ok((item, markers))
}

struct ExposeDeclaration {
    name: LitStr,
    authority: Option<Expr>,
}

fn expose_declaration(
    attribute: &Attribute,
    method: &Ident,
) -> syn::Result<Option<ExposeDeclaration>> {
    let Meta::List(meta) = &attribute.meta else {
        return Ok(None);
    };
    let outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let Some(first) = outer.first() else {
        return Ok(None);
    };

    let (is_expose, modifiers) = match first {
        Meta::Path(path) if path.is_ident("expose") => {
            (true, outer.iter().skip(1).cloned().collect::<Vec<_>>())
        }
        Meta::List(list) if list.path.is_ident("expose") => {
            if outer.len() != 1 {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "expose metadata belongs inside expose(...) when that form is used",
                ));
            }
            let inner = Punctuated::<Meta, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())?
                .into_iter()
                .collect::<Vec<_>>();
            (true, inner)
        }
        _ => (false, Vec::new()),
    };
    if !is_expose {
        return Ok(None);
    }

    let mut name = None;
    let mut authority = None;
    for modifier in modifiers {
        let Meta::NameValue(value) = modifier else {
            return Err(syn::Error::new_spanned(
                modifier,
                "expose metadata must use name = \"...\" or authority = <expression>",
            ));
        };
        if value.path.is_ident("name") {
            if name.is_some() {
                return Err(syn::Error::new_spanned(value, "duplicate expose name"));
            }
            let name_value = match value.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) => value,
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "expose name must be a string literal",
                    ));
                }
            };
            validate_public_segment(&name_value)?;
            name = Some(name_value);
        } else if value.path.is_ident("authority") {
            if authority.is_some() {
                return Err(syn::Error::new_spanned(value, "duplicate expose authority"));
            }
            authority = Some(value.value);
        } else {
            return Err(syn::Error::new_spanned(
                value.path,
                "unsupported expose metadata",
            ));
        }
    }

    let name = name.unwrap_or_else(|| LitStr::new(&method.to_string(), method.span()));
    validate_public_segment(&name)?;
    Ok(Some(ExposeDeclaration { name, authority }))
}

fn validate_public_segment(value: &LitStr) -> syn::Result<()> {
    let name = value.value();
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(syn::Error::new_spanned(
            value,
            "public member name must be one non-empty alphanumeric, '_' or '-' path segment",
        ));
    }
    Ok(())
}

fn normalize_on_event(attribute: &mut Attribute) -> syn::Result<()> {
    let Meta::List(meta) = &attribute.meta else {
        return Ok(());
    };
    let mut outer = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let Some(first) = outer.first_mut() else {
        return Ok(());
    };
    let Meta::List(kind) = first else {
        return Ok(());
    };
    if !kind.path.is_ident("on_event") {
        return Ok(());
    }
    kind.path = parse_quote!(listen);
    attribute.meta = syn::parse2(quote!(phenix(#outer)))?;
    Ok(())
}

fn retarget_behavior(
    expanded: TokenStream,
    root: &Ident,
    plugin: &Type,
) -> syn::Result<TokenStream> {
    let mut file = syn::parse2::<syn::File>(expanded)?;
    let target: Type = parse_quote!(#root);
    let mut found = false;
    for item in &mut file.items {
        let Item::Impl(item) = item else {
            continue;
        };
        if trait_is(item, "StaticComponentBehavior") {
            *item.self_ty = target.clone();
            let exports = item.items.iter_mut().find_map(|member| match member {
                ImplItem::Fn(method) if method.sig.ident == "exports" => Some(method),
                _ => None,
            });
            let Some(exports) = exports else {
                return Err(syn::Error::new_spanned(
                    item,
                    "plugin root export metadata was not generated",
                ));
            };
            let direct = exports.block.clone();
            exports.block = parse_quote!({
                let mut exports = (|| #direct)();
                exports.extend(
                    <#plugin as ::phenix_sdk::StaticRootExpose>::root_exposed_field_exports(),
                );
                exports
            });
            found = true;
        }
        if trait_is(item, "StaticComponentRuntimeDispatch") {
            let dispatch = item.items.iter_mut().find_map(|member| match member {
                ImplItem::Fn(method) if method.sig.ident == "dispatch_runtime" => Some(method),
                _ => None,
            });
            let Some(dispatch) = dispatch else {
                return Err(syn::Error::new_spanned(
                    item,
                    "plugin root runtime dispatch was not generated",
                ));
            };
            let direct = dispatch.block.clone();
            dispatch.block = parse_quote!({
                if let Some(result) =
                    <#plugin as ::phenix_sdk::StaticRootExpose>::dispatch_root_exposed_field(
                        self,
                        service,
                        input,
                        host,
                    )
                {
                    return result;
                }
                #direct
            });
        }
    }
    if !found {
        return Err(syn::Error::new_spanned(
            root,
            "plugin root behavior metadata was not generated",
        ));
    }
    Ok(quote!(#file))
}

fn trait_is(item: &ItemImpl, expected: &str) -> bool {
    item.trait_
        .as_ref()
        .and_then(|(_, path, _)| path.segments.last())
        .is_some_and(|segment| segment.ident == expected)
}

fn has_root_behavior(item: &ItemImpl) -> syn::Result<bool> {
    for member in &item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        for attribute in &method.attrs {
            if !attribute.path().is_ident("phenix") {
                continue;
            }
            if lifecycle_role(attribute)?.is_none() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn lifecycle_role(attribute: &Attribute) -> syn::Result<Option<&'static str>> {
    if !attribute.path().is_ident("phenix") {
        return Ok(None);
    }
    let Meta::List(meta) = &attribute.meta else {
        return Ok(None);
    };
    let roles = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    if roles.len() != 1 {
        return Ok(None);
    }
    let Some(Meta::Path(role)) = roles.first() else {
        return Ok(None);
    };
    if role.is_ident("start") {
        Ok(Some("start"))
    } else if role.is_ident("stop") {
        Ok(Some("stop"))
    } else {
        Ok(None)
    }
}

fn self_type_ident(item: &ItemImpl) -> syn::Result<Ident> {
    let Type::Path(path) = item.self_ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            &item.self_ty,
            "plugin behavior requires a concrete path self type",
        ));
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.clone())
        .ok_or_else(|| syn::Error::new_spanned(&item.self_ty, "plugin self type is empty"))
}

fn root_component_ident(plugin: &Ident) -> Ident {
    format_ident!("__PhenixPluginRootComponentFor{}", plugin)
}

fn expose_marker_ident(plugin: &Ident, method: &Ident) -> Ident {
    format_ident!("__PhenixExposed{}_{}", plugin, method)
}

fn should_inject_root(
    args: &TokenStream,
    item: &ItemStruct,
    root_requested: bool,
) -> syn::Result<bool> {
    if !root_requested {
        return Ok(false);
    }
    if !matches!(item.fields, Fields::Named(_)) {
        return Err(syn::Error::new_spanned(
            item,
            "plugin root behavior requires named struct fields",
        ));
    }
    if has_root_fields(item)? {
        return Err(syn::Error::new_spanned(
            item,
            "plugin root behavior cannot be combined with structural root component fields",
        ));
    }
    if !embedded_execution(args.clone())? {
        return Err(syn::Error::new_spanned(
            item,
            "plugin root behavior requires embedded execution",
        ));
    }
    Ok(true)
}

struct ExposeRemap {
    from: LitStr,
    to: LitStr,
}

fn split_root_arguments(args: TokenStream) -> syn::Result<(TokenStream, bool, Vec<ExposeRemap>)> {
    if args.is_empty() || syn::parse2::<LitStr>(args.clone()).is_ok() {
        return Ok((args, false, Vec::new()));
    }

    let arguments = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args.clone())?;
    let mut root = false;
    let mut remaps = Vec::new();
    let mut retained = Punctuated::<Meta, Token![,]>::new();
    for argument in arguments {
        if matches!(&argument, Meta::Path(path) if path.is_ident("root")) {
            if root {
                return Err(syn::Error::new_spanned(
                    argument,
                    "duplicate plugin root flag",
                ));
            }
            root = true;
            continue;
        }
        if matches!(&argument, Meta::List(list) if list.path.is_ident("remap")) {
            let Meta::List(remap) = argument else {
                unreachable!("remap match requires list metadata")
            };
            remaps.push(parse_expose_remap(remap)?);
            continue;
        }
        retained.push(argument);
    }
    if !remaps.is_empty() && !root {
        return Err(syn::Error::new_spanned(
            args,
            "public path remapping requires plugin root behavior",
        ));
    }
    validate_expose_remaps(&remaps)?;
    Ok((quote!(#retained), root, remaps))
}

fn parse_expose_remap(remap: syn::MetaList) -> syn::Result<ExposeRemap> {
    let arguments = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(remap.tokens.clone())?;
    let mut from = None;
    let mut to = None;
    for argument in arguments {
        let Meta::NameValue(value) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "remap metadata must use from = \"...\" and to = \"...\"",
            ));
        };
        let path = match value.value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(value),
                ..
            }) => value,
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "remap paths must be string literals",
                ));
            }
        };
        validate_public_path(&path)?;
        if value.path.is_ident("from") {
            if from.replace(path).is_some() {
                return Err(syn::Error::new_spanned(
                    value.path,
                    "duplicate remap source",
                ));
            }
        } else if value.path.is_ident("to") {
            if to.replace(path).is_some() {
                return Err(syn::Error::new_spanned(
                    value.path,
                    "duplicate remap destination",
                ));
            }
        } else {
            return Err(syn::Error::new_spanned(
                value.path,
                "remap metadata supports only from and to",
            ));
        }
    }
    let from = from.ok_or_else(|| syn::Error::new_spanned(&remap.path, "remap requires from"))?;
    let to = to.ok_or_else(|| syn::Error::new_spanned(&remap.path, "remap requires to"))?;
    Ok(ExposeRemap { from, to })
}

fn validate_expose_remaps(remaps: &[ExposeRemap]) -> syn::Result<()> {
    let mut sources = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    for remap in remaps {
        if !sources.insert(remap.from.value()) {
            return Err(syn::Error::new_spanned(
                &remap.from,
                "duplicate remap source creates an ambiguous equal-specificity declaration",
            ));
        }
        if !destinations.insert(remap.to.value()) {
            return Err(syn::Error::new_spanned(
                &remap.to,
                "duplicate remap destination creates an ambiguous reverse declaration",
            ));
        }
    }
    Ok(())
}

fn validate_public_path(value: &LitStr) -> syn::Result<()> {
    let path = value.value();
    if path.is_empty()
        || path.split('/').any(|segment| {
            segment.is_empty()
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
    {
        return Err(syn::Error::new_spanned(
            value,
            "public path must contain non-empty alphanumeric, '_' or '-' segments",
        ));
    }
    Ok(())
}

fn embedded_execution(args: TokenStream) -> syn::Result<bool> {
    if args.is_empty() || syn::parse2::<LitStr>(args.clone()).is_ok() {
        return Ok(true);
    }
    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    for argument in args {
        let Meta::NameValue(argument) = argument else {
            continue;
        };
        if !argument.path.is_ident("execution") {
            continue;
        }
        return Ok(match argument.value {
            Expr::Path(path) => path
                .path
                .segments
                .last()
                .is_none_or(|segment| segment.ident != "ResourceOnly"),
            Expr::Struct(_) => false,
            _ => true,
        });
    }
    Ok(true)
}

fn has_root_fields(item: &ItemStruct) -> syn::Result<bool> {
    let Fields::Named(fields) = &item.fields else {
        return Ok(false);
    };
    for field in &fields.named {
        for attribute in &field.attrs {
            if !attribute.path().is_ident("phenix") {
                continue;
            }
            let Meta::List(meta) = &attribute.meta else {
                continue;
            };
            let arguments =
                Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
            let Some(first) = arguments.first() else {
                continue;
            };
            match first {
                Meta::Path(path) if path.is_ident("import") || path.is_ident("host") => {
                    return Ok(true)
                }
                Meta::List(list) if list.path.is_ident("event") => return Ok(true),
                _ => {}
            }
        }
    }
    Ok(false)
}

fn plugin_id_literal(args: TokenStream, item: &Ident) -> syn::Result<LitStr> {
    if let Ok(value) = syn::parse2::<LitStr>(args.clone()) {
        return Ok(value);
    }
    if !args.is_empty() {
        let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
        for argument in args {
            let Meta::NameValue(argument) = argument else {
                continue;
            };
            if !argument.path.is_ident("id") {
                continue;
            }
            let value = match argument.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) => value,
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "plugin id must be a string literal",
                    ));
                }
            };
            return Ok(value);
        }
    }

    let package = std::env::var("CARGO_PKG_NAME")
        .map_err(|_| syn::Error::new_spanned(item, "plugin package identity is unavailable"))?;
    let name = package
        .strip_prefix("phenix-plugin-")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            syn::Error::new_spanned(
                item,
                "plugin without an explicit id requires a phenix-plugin-* package name",
            )
        })?;
    Ok(LitStr::new(&format!("phenix.{name}"), item.span()))
}

fn strip_synthetic_root(expanded: TokenStream, plugin: &Ident) -> syn::Result<TokenStream> {
    let mut file = syn::parse2::<syn::File>(expanded)?;
    for item in &mut file.items {
        let Item::Struct(item) = item else {
            continue;
        };
        if &item.ident != plugin {
            continue;
        }
        let Fields::Named(fields) = &mut item.fields else {
            continue;
        };
        fields.named = fields
            .named
            .clone()
            .into_iter()
            .filter(|field| field.ident.as_ref().is_none_or(|ident| ident != ROOT_FIELD))
            .collect();
        break;
    }

    Ok(quote!(#file))
}

fn component_id(component: &ComponentField) -> TokenStream {
    let field = &component.field;
    let ty = &component.ty;
    match &component.id {
        Some(id) => quote! {
            ::phenix_sdk::StaticComponentDescriptor::explicit::<#ty>(
                #id,
                stringify!(#field),
            )
            .id
        },
        None => quote! {
            ::phenix_sdk::StaticComponentDescriptor::derived::<#ty>(
                &Self::plugin_id(),
                stringify!(#field),
            )
            .id
        },
    }
}

fn append_lifecycle_runtime(expanded: TokenStream, item: &ItemImpl) -> syn::Result<TokenStream> {
    let (start, stop) = lifecycle_methods(item)?;
    let self_ty = &item.self_ty;

    let start_adapter = start.as_ref().map(|method| {
        quote! {
            #[doc(hidden)]
            fn __phenix_runtime_start(
                &mut self,
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<(), String> {
                let context = ::phenix_sdk::PluginContext::new(host, (), (), ());
                self.#method(&context).map_err(|error| error.to_string())
            }
        }
    });
    let stop_adapter = stop.as_ref().map(|method| {
        quote! {
            #[doc(hidden)]
            fn __phenix_runtime_stop(
                &mut self,
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<(), String> {
                let context = ::phenix_sdk::PluginContext::new(host, (), (), ());
                self.#method(&context).map_err(|error| error.to_string())
            }
        }
    });
    let with_start = start.as_ref().map(|_| {
        quote! {
            let instance = instance.with_start(Self::__phenix_runtime_start);
        }
    });
    let with_stop = stop.as_ref().map(|_| {
        quote! {
            let instance = instance.with_stop(Self::__phenix_runtime_stop);
        }
    });

    Ok(quote! {
        #expanded

        impl #self_ty {
            #start_adapter
            #stop_adapter

            #[doc(hidden)]
            pub fn __phenix_into_plugin_instance(
                self,
            ) -> Box<dyn ::phenix_sdk::__phenix_plugin::PluginInstance>
            where
                Self: ::phenix_sdk::StaticPluginComponentDispatch
                    + ::phenix_sdk::StaticPluginResources
                    + Send
                    + Sync
                    + 'static,
            {
                let instance = ::phenix_sdk::StaticPluginInstance::from_component_dispatch(self);
                #with_start
                #with_stop
                Box::new(instance)
            }
        }
    })
}

fn validate_lifecycle_runtime_abi(item: &ItemImpl) -> syn::Result<()> {
    for member in &item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        if !has_lifecycle_role(method)? {
            continue;
        }
        if method.sig.unsafety.is_some()
            || method.sig.abi.is_some()
            || method.sig.variadic.is_some()
        {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "plugin lifecycle methods must use the ordinary safe Rust ABI",
            ));
        }
    }
    Ok(())
}

fn has_lifecycle_role(method: &syn::ImplItemFn) -> syn::Result<bool> {
    for attribute in &method.attrs {
        if lifecycle_role(attribute)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn lifecycle_methods(item: &ItemImpl) -> syn::Result<(Option<syn::Ident>, Option<syn::Ident>)> {
    let mut start = None;
    let mut stop = None;
    for member in &item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        for attribute in &method.attrs {
            match lifecycle_role(attribute)? {
                Some("start") => start = Some(method.sig.ident.clone()),
                Some("stop") => stop = Some(method.sig.ident.clone()),
                _ => {}
            }
        }
    }
    Ok((start, stop))
}

struct ComponentField {
    field: syn::Ident,
    ty: Type,
    id: Option<syn::LitStr>,
}

fn component_fields(item: &ItemStruct) -> syn::Result<Vec<ComponentField>> {
    let Fields::Named(fields) = &item.fields else {
        return Ok(Vec::new());
    };

    let mut components = Vec::new();
    for field in &fields.named {
        for attribute in &field.attrs {
            if !attribute.path().is_ident("phenix") {
                continue;
            }
            let Meta::List(meta) = &attribute.meta else {
                continue;
            };
            let arguments =
                Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
            let Some(Meta::Path(role)) = arguments.first() else {
                continue;
            };
            if !role.is_ident("component") {
                continue;
            }

            let id = arguments.iter().skip(1).find_map(|argument| {
                let Meta::NameValue(value) = argument else {
                    return None;
                };
                if !value.path.is_ident("id") {
                    return None;
                }
                let Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) = &value.value
                else {
                    return None;
                };
                Some(value.clone())
            });
            components.push(ComponentField {
                field: field.ident.clone().expect("named field has an identifier"),
                ty: field.ty.clone(),
                id,
            });
        }
    }
    Ok(components)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn lifecycle_impl_wires_runtime_callbacks_into_stateful_adapter() {
        let output = expand(
            TokenStream::new(),
            quote! {
                impl Plugin {
                    #[phenix(start)]
                    fn activate(
                        &mut self,
                        _context: &phenix_sdk::PluginContext<'_, '_, ()>,
                    ) -> Result<(), String> {
                        Ok(())
                    }

                    #[phenix(stop)]
                    fn deactivate(
                        &mut self,
                        _context: &phenix_sdk::PluginContext<'_, '_, ()>,
                    ) -> Result<(), String> {
                        Ok(())
                    }
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("__phenix_runtime_start"));
        assert!(output.contains("__phenix_runtime_stop"));
        assert!(output.contains("with_start"));
        assert!(output.contains("with_stop"));
        assert!(output.contains("__phenix_into_plugin_instance"));
    }

    #[test]
    fn root_impl_lowers_expose_and_on_event_without_api_redirect() {
        let output = expand(
            TokenStream::new(),
            quote! {
                impl Plugin {
                    #[phenix(expose(name = "run"))]
                    async fn execute(&self, request: Request) -> Response {
                        todo!()
                    }

                    #[phenix(on_event("fixture.changed"))]
                    async fn changed(
                        &self,
                        _context: &phenix_sdk::EventContext,
                        _event: Event,
                    ) {
                    }
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticComponentBehavior for __PhenixPluginRootComponentForPlugin"));
        assert!(output.contains("StaticComponentRuntimeDispatch for Plugin"));
        assert!(output.contains("root_exposed_interface"));
        assert!(output.contains("fixture.changed"));
        assert!(!output.contains("on_event"));
    }

    #[test]
    fn root_flag_generates_hidden_default_component_without_struct_field() {
        let output = expand(
            quote!(root, id = "fixture.root"),
            quote! {
                struct Plugin {
                    state: usize,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("__PhenixPluginRootComponentForPlugin"));
        assert!(output.contains("fixture.root"));
        assert!(!output.contains("__phenix_root :"));
    }

    #[test]
    fn root_struct_collects_exposed_fields_and_remaps() {
        let output = expand(
            quote!(
                root,
                id = "fixture.root",
                remap(from = "models/current", to = "api/model")
            ),
            quote! {
                struct Plugin {
                    #[phenix(expose(name = "models"))]
                    repository: Repository,
                    #[phenix(component)]
                    worker: Worker,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticExposeFields for Plugin"));
        assert!(output.contains("StaticRootExpose for Plugin"));
        assert!(output.contains("models/current"));
        assert!(output.contains("api/model"));
        assert!(output.contains("worker"));
        assert!(!output.contains("phenix (expose"));
    }

    #[test]
    fn root_remaps_reject_duplicate_sources_and_destinations() {
        let source = expand(
            quote!(
                root,
                id = "fixture.root",
                remap(from = "old", to = "first"),
                remap(from = "old", to = "second")
            ),
            quote!(
                struct Plugin {}
            ),
        )
        .unwrap_err();
        assert!(source.to_string().contains("duplicate remap source"));

        let destination = expand(
            quote!(
                root,
                id = "fixture.root",
                remap(from = "first", to = "new"),
                remap(from = "second", to = "new")
            ),
            quote!(
                struct Plugin {}
            ),
        )
        .unwrap_err();
        assert!(destination
            .to_string()
            .contains("duplicate remap destination"));
    }

    #[test]
    fn root_impl_rejects_duplicate_exposed_method_names() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Plugin {
                    #[phenix(expose(name = "same"))]
                    fn first(&mut self) {}

                    #[phenix(expose(name = "same"))]
                    fn second(&mut self) {}
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("duplicate exposed method path"));
    }

    #[test]
    fn lifecycle_impl_rejects_unsafe_runtime_abi() {
        let error = expand(
            TokenStream::new(),
            quote! {
                impl Plugin {
                    #[phenix(start)]
                    unsafe fn activate(
                        &mut self,
                        _context: &phenix_sdk::PluginContext<'_, '_, ()>,
                    ) -> Result<(), String> {
                        Ok(())
                    }
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("ordinary safe Rust ABI"));
    }
}
