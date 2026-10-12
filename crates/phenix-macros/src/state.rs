use crate::{args, no_extra, required};
use quote::quote;
use syn::{parse::Parser, spanned::Spanned, Expr, ItemMod, Meta, Type};

fn declarations(tokens: proc_macro2::TokenStream) -> syn::Result<Vec<Meta>> {
    Ok(
        syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated
            .parse2(tokens)?
            .into_iter()
            .collect(),
    )
}
pub fn selected(tokens: proc_macro2::TokenStream) -> syn::Result<bool> {
    let mut contributions = false;
    for meta in declarations(tokens)? {
        if let Meta::List(list) = &meta {
            contributions |= ["define", "require", "provide"]
                .iter()
                .any(|name| list.path.is_ident(name));
        }
        if let Meta::NameValue(value) = meta {
            if value.path.is_ident("profile") {
                if let Expr::Lit(value) = value.value {
                    if let syn::Lit::Str(text) = value.lit {
                        if text.value() == "state" {
                            return Ok(true);
                        }
                        if text.value() == "definitions" {
                            return Ok(false);
                        }
                    }
                }
            }
        }
    }
    Ok(contributions)
}

// Rust's type checker validates injected arguments against the declaration.
// Only the roles have syntax rules; response conversion stays type directed.
fn arguments(
    signature: &syn::Signature,
    request_allowed: bool,
    host_allowed: bool,
) -> syn::Result<(proc_macro2::TokenStream, bool)> {
    let mut roles = std::collections::BTreeSet::new();
    let mut arguments = Vec::new();
    let mut has_request = false;
    for argument in &signature.inputs {
        let syn::FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new(
                argument.span(),
                "native handler cannot have self",
            ));
        };
        let (role, value) = match argument.ty.as_ref() {
            Type::Reference(reference) => {
                let Type::Path(path) = reference.elem.as_ref() else {
                    return Err(syn::Error::new(
                        reference.span(),
                        "handler references are State or Host",
                    ));
                };
                match path
                    .path
                    .segments
                    .last()
                    .map(|segment| segment.ident.to_string())
                    .as_deref()
                {
                    Some("State") => ("state", quote!(state)),
                    Some("Host") if host_allowed && reference.mutability.is_none() => {
                        ("host", quote!(&host))
                    }
                    _ => {
                        return Err(syn::Error::new(
                            reference.span(),
                            "handler references are State or shared Host",
                        ))
                    }
                }
            }
            _ if request_allowed => {
                has_request = true;
                ("request", quote!(request))
            }
            _ => {
                return Err(syn::Error::new(
                    argument.span(),
                    "hook accepts only State and Host",
                ))
            }
        };
        if !roles.insert(role) {
            return Err(syn::Error::new(
                argument.span(),
                "duplicate handler argument role",
            ));
        }
        arguments.push(value);
    }
    Ok((quote!(#(#arguments),*), has_request))
}

pub fn expand(
    tokens: proc_macro2::TokenStream,
    mut module: ItemMod,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut attributes = std::collections::BTreeMap::new();
    let mut definitions = Vec::new();
    let mut requirements = Vec::new();
    let mut exports = Vec::new();
    let mut provided = Vec::new();
    let mut imports = Vec::new();
    let mut slots = std::collections::BTreeSet::new();
    for meta in declarations(tokens)? {
        if let Meta::List(list) = &meta {
            if list.path.is_ident("define") {
                let api: syn::Path = syn::parse2(list.tokens.clone())?;
                definitions.push(quote!(#api::CONTRACT));
                continue;
            }
            let require = list.path.is_ident("require");
            if !require && !list.path.is_ident("provide") {
                return Err(syn::Error::new(list.span(), "unknown state contribution"));
            }
            let parts = declarations(list.tokens.clone())?;
            let Some(Meta::Path(api)) = parts.first() else {
                return Err(syn::Error::new(
                    list.span(),
                    "contribution needs a contract path",
                ));
            };
            let mut options = std::collections::BTreeMap::new();
            let mut cardinality = quote!(::phenix_sdk::Cardinality::One);
            let mut policy = false;
            let mut indexed = false;
            for part in parts.iter().skip(1) {
                match part {
                    Meta::Path(path) if require && path.is_ident("optional") && !policy => {
                        cardinality = quote!(::phenix_sdk::Cardinality::Optional);
                        policy = true;
                    }
                    Meta::List(many) if require && many.path.is_ident("many") && !policy => {
                        let minimum: Meta = syn::parse2(many.tokens.clone())?;
                        let Meta::NameValue(minimum) = minimum else {
                            return Err(syn::Error::new(many.span(), "many needs min = integer"));
                        };
                        let Expr::Lit(value) = minimum.value else {
                            return Err(syn::Error::new(many.span(), "many needs min = integer"));
                        };
                        let syn::Lit::Int(value) = value.lit else {
                            return Err(syn::Error::new(many.span(), "many needs min = integer"));
                        };
                        let n: u64 = value.base10_parse()?;
                        if !minimum.path.is_ident("min") || n > phenix_contract::MAX_COUNT as u64 {
                            return Err(syn::Error::new(many.span(), "invalid Many minimum"));
                        }
                        cardinality = quote!(::phenix_sdk::Cardinality::Many { minimum: #n });
                        policy = true;
                        indexed = true;
                    }
                    Meta::NameValue(_) => {
                        for (key, value) in args(quote!(#part))? {
                            if options.insert(key, value).is_some() {
                                return Err(syn::Error::new(part.span(), "duplicate slot option"));
                            }
                        }
                    }
                    _ => return Err(syn::Error::new(part.span(), "invalid slot policy")),
                }
            }
            let slot = required(&mut options, "slot")?;
            no_extra(&options)?;
            let symbol: syn::Ident = syn::parse_str(&slot).map_err(|_| {
                syn::Error::new(list.span(), "slot symbol must be a Rust identifier")
            })?;
            if !slots.insert((require, slot.clone())) {
                return Err(syn::Error::new(list.span(), "duplicate slot"));
            }
            if require {
                let selector = if indexed {
                    format!("{slot}#0")
                } else {
                    slot.clone()
                };
                let invoke_at = if indexed {
                    quote! {
                        pub fn invoke_at<O: #api::Member>(host: &::phenix_sdk::live::Host<'_>, index: usize, request: &O::Request) -> ::phenix_sdk::CallResult<O::Response, O::Error> {
                            host.invoke::<O>(&format!("{}#{}", #slot, index), request)
                        }
                    }
                } else {
                    quote!()
                };
                requirements.push(quote!(::phenix_sdk::StaticImport { slot: #slot, contract: #api::CONTRACT.contract, revision_min: #api::CONTRACT.revision, revision_max: #api::CONTRACT.revision, cardinality: #cardinality }));
                imports.push(quote! {
                    #[allow(non_camel_case_types)] pub struct #symbol;
                    impl #symbol {
                        pub fn invoke<O: #api::Member>(host: &::phenix_sdk::live::Host<'_>, request: &O::Request) -> ::phenix_sdk::CallResult<O::Response, O::Error> { host.invoke::<O>(#selector, request) }
                        #invoke_at
                        pub fn count(host: &::phenix_sdk::live::Host<'_>) -> Result<usize, ::phenix_sdk::Failure> { host.slot_count(#slot) }
                    }
                });
            } else {
                exports.push(quote!(::phenix_sdk::StaticExport { slot: #slot, contract: #api::CONTRACT.contract, revision: #api::CONTRACT.revision }));
                provided.push(quote!(#api::CONTRACT));
            }
        } else {
            for (key, value) in args(quote!(#meta))? {
                if attributes.insert(key, value).is_some() {
                    return Err(syn::Error::new(meta.span(), "duplicate plugin attribute"));
                }
            }
        }
    }
    let id = required(&mut attributes, "id")?;
    let version = attributes
        .remove("version")
        .unwrap_or_else(|| "0.1.0".into());
    let profile = attributes
        .remove("profile")
        .unwrap_or_else(|| "state".into());
    no_extra(&attributes)?;
    if profile != "state"
        || id == "root"
        || !phenix_contract::identifier(&id)
        || !phenix_contract::identifier(&version)
    {
        return Err(syn::Error::new(
            module.span(),
            "invalid state plugin identity",
        ));
    }
    let (_, items) = module.content.as_mut().ok_or_else(|| {
        syn::Error::new(module.ident.span(), "state plugin needs an inline module")
    })?;
    let mut state = false;
    let mut properties = Vec::new();
    let mut property_symbols = Vec::new();
    let mut bindings = Vec::new();
    let mut property_ids = std::collections::BTreeSet::new();
    let mut handlers = Vec::new();
    let mut synchronous = Vec::new();
    let mut pending = Vec::new();
    let mut polls = Vec::new();
    let mut cancels = Vec::new();
    let mut start = None;
    let mut stop = None;
    for item in items.iter_mut() {
        match item {
            syn::Item::Struct(s) if s.ident == "State" => {
                if state
                    || !s.generics.params.is_empty()
                    || !matches!(s.fields, syn::Fields::Named(_))
                {
                    return Err(syn::Error::new(
                        s.span(),
                        "State must be a unique nongeneric named struct",
                    ));
                }
                state = true;
                for field in &mut s.fields {
                    let mut access = [false; 3];
                    let mut stable = None;
                    let mut keep = Vec::new();
                    for attr in field.attrs.drain(..) {
                        if let Some(index) = ["get", "set", "watch"]
                            .iter()
                            .position(|name| attr.path().is_ident(name))
                        {
                            if access[index] {
                                return Err(syn::Error::new(
                                    attr.span(),
                                    "duplicate field operation",
                                ));
                            }
                            access[index] = true;
                            if let Meta::List(list) = &attr.meta {
                                let mut options = args(list.tokens.clone())?;
                                let id = required(&mut options, "id")?;
                                no_extra(&options)?;
                                if let Some(previous) = &stable {
                                    if previous != &id {
                                        return Err(syn::Error::new(
                                            attr.span(),
                                            "conflicting field identities",
                                        ));
                                    }
                                }
                                stable = Some(id);
                            } else if !matches!(attr.meta, Meta::Path(_)) {
                                return Err(syn::Error::new(
                                    attr.span(),
                                    "invalid field operation",
                                ));
                            }
                        } else {
                            keep.push(attr);
                        }
                    }
                    field.attrs = keep;
                    if access.iter().any(|v| *v) {
                        if field.attrs.iter().any(|a| !a.path().is_ident("doc")) {
                            return Err(syn::Error::new(
                                field.span(),
                                "exposed fields must be unconditional",
                            ));
                        }
                        let name = field.ident.as_ref().unwrap();
                        let stable = stable
                            .unwrap_or_else(|| name.to_string().trim_start_matches("r#").into());
                        if !property_ids.insert(stable.clone()) {
                            return Err(syn::Error::new(
                                field.span(),
                                "duplicate property identity",
                            ));
                        }
                        let ty = field.ty.clone();
                        let [get, set, watch] = access;
                        properties.push(quote!(::phenix_sdk::StaticProperty { id: #stable, schema: <#ty as ::phenix_sdk::live::Portable>::SCHEMA, get: #get, set: #set, watch: #watch }));
                        property_symbols.push(crate::property::symbols(
                            &id,
                            &stable,
                            name,
                            &syn::parse_quote!(pub),
                            &ty,
                            access,
                        )?);
                        bindings.push(quote!(state.#name.bind(&host, #stable).map_err(|error| match error { ::phenix_sdk::live::AccessError::Kernel(error) => error, ::phenix_sdk::live::AccessError::State(error) => ::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed, format!("property initialization: {error:?}")) })?;));
                        field.ty = syn::parse_quote!(::phenix_sdk::live::Cell<#ty>);
                    }
                }
            }
            syn::Item::Fn(function) => {
                let mut keep = Vec::new();
                let mut annotated = false;
                for attr in function.attrs.drain(..) {
                    if ["provide", "pending", "poll", "cancel", "start", "stop"]
                        .iter()
                        .any(|name| attr.path().is_ident(name))
                    {
                        if annotated
                            || function.sig.asyncness.is_some()
                            || function.sig.unsafety.is_some()
                            || !function.sig.generics.params.is_empty()
                        {
                            return Err(syn::Error::new(
                                attr.span(),
                                "native hook must have one synchronous annotation",
                            ));
                        }
                        annotated = true;
                        let name = function.sig.ident.clone();
                        let request_allowed =
                            attr.path().is_ident("provide") || attr.path().is_ident("pending");
                        let lifecycle =
                            attr.path().is_ident("start") || attr.path().is_ident("stop");
                        let (arguments, has_request) =
                            arguments(&function.sig, request_allowed, !lifecycle)?;
                        if attr.path().is_ident("start") || attr.path().is_ident("stop") {
                            if !matches!(attr.meta, Meta::Path(_)) {
                                return Err(syn::Error::new(
                                    attr.span(),
                                    "lifecycle annotation takes no arguments",
                                ));
                            }
                            let selected = if attr.path().is_ident("start") {
                                &mut start
                            } else {
                                &mut stop
                            };
                            if selected.replace((name, arguments)).is_some() {
                                return Err(syn::Error::new(
                                    attr.span(),
                                    "duplicate lifecycle hook",
                                ));
                            }
                        } else {
                            let operation: Type = attr.parse_args()?;
                            if attr.path().is_ident("provide") || attr.path().is_ident("pending") {
                                handlers.push(quote!(::phenix_sdk::StaticOperation { name: "handler", key: <#operation as ::phenix_sdk::live::Operation>::KEY, request: <<#operation as ::phenix_sdk::live::Operation>::Request as ::phenix_sdk::live::Portable>::SCHEMA, response: <<#operation as ::phenix_sdk::live::Operation>::Response as ::phenix_sdk::live::Portable>::SCHEMA, error: <<#operation as ::phenix_sdk::live::Operation>::Error as ::phenix_sdk::live::Portable>::SCHEMA }));
                                if attr.path().is_ident("provide") {
                                    synchronous.push((operation, name, arguments, has_request));
                                } else {
                                    pending.push((operation, name, arguments, has_request));
                                }
                            } else if attr.path().is_ident("poll") {
                                polls.push((operation, name, arguments));
                            } else {
                                cancels.push((operation, name, arguments));
                            }
                        }
                    } else {
                        keep.push(attr);
                    }
                }
                function.attrs = keep;
                if annotated && function.attrs.iter().any(|a| !a.path().is_ident("doc")) {
                    return Err(syn::Error::new(
                        function.span(),
                        "native hooks must be unconditional",
                    ));
                }
            }
            _ => {}
        }
    }
    if exports.is_empty() && properties.is_empty() {
        return Err(syn::Error::new(
            module.ident.span(),
            "plugin needs a service or property export",
        ));
    }
    if !state {
        items.push(syn::parse_quote!(
            #[derive(Default)]
            struct State {}
        ));
    }
    if pending.len() != polls.len() || pending.len() != cancels.len() {
        return Err(syn::Error::new(
            module.ident.span(),
            "pending exports need matching poll and cancel hooks",
        ));
    }
    let mut invoke = Vec::new();
    let mut poll = Vec::new();
    let mut cancel = Vec::new();
    for (op, name, arguments, has_request) in synchronous {
        let unit_request = (!has_request).then(|| quote!(let _: () = request;));
        invoke.push(quote! { if key == <#op as ::phenix_sdk::live::Operation>::KEY {
            let request = <<#op as ::phenix_sdk::live::Operation>::Request as ::phenix_sdk::live::Portable>::from_value(value)?;
            #unit_request
            let result = ::phenix_sdk::live::handler_result::<<#op as ::phenix_sdk::live::Operation>::Response, <#op as ::phenix_sdk::live::Operation>::Error, _>(#name(#arguments));
            return Ok(::phenix_sdk::live::reply(result));
        }});
    }
    for (op, name, arguments, has_request) in pending {
        let unit_request = (!has_request).then(|| quote!(let _: () = request;));
        let matching = |hooks: &[(Type, syn::Ident, proc_macro2::TokenStream)]| -> syn::Result<(syn::Ident, proc_macro2::TokenStream)> {
            let matching: Vec<_> = hooks
                .iter()
                .filter(|(ty, _, _)| quote!(#ty).to_string() == quote!(#op).to_string())
                .collect();
            if matching.len() != 1 {
                return Err(syn::Error::new(
                    op.span(),
                    "pending export needs one matching hook",
                ));
            }
            Ok((matching[0].1.clone(), matching[0].2.clone()))
        };
        let (poll_name, poll_arguments) = matching(&polls)?;
        let (cancel_name, cancel_arguments) = matching(&cancels)?;
        invoke.push(quote! { if key == <#op as ::phenix_sdk::live::Operation>::KEY {
            if api.ticket == 0 {
                return Ok(::phenix_sdk::abi::Reply::state_value(5, ::phenix_sdk::Failure::new(::phenix_sdk::Code::AbiIncompatible, "pending operation requires an existing native ticket").to_value()));
            }
            let request = <<#op as ::phenix_sdk::live::Operation>::Request as ::phenix_sdk::live::Portable>::from_value(value)?;
            #unit_request
            let result: ::phenix_sdk::live::PendingResult<<#op as ::phenix_sdk::live::Operation>::Response, <#op as ::phenix_sdk::live::Operation>::Error> = #name(#arguments);
            return Ok(::phenix_sdk::live::pending_reply(result));
        }});
        poll.push(quote! { if key == <#op as ::phenix_sdk::live::Operation>::KEY {
            let result: ::phenix_sdk::live::PendingResult<<#op as ::phenix_sdk::live::Operation>::Response, <#op as ::phenix_sdk::live::Operation>::Error> = #poll_name(#poll_arguments);
            return Ok(::phenix_sdk::live::pending_reply(result));
        }});
        cancel.push(quote! { if key == <#op as ::phenix_sdk::live::Operation>::KEY {
            let result: Result<bool, ::phenix_sdk::Failure> = #cancel_name(#cancel_arguments);
            return Ok(::phenix_sdk::abi::Reply::state_value(if result? { 4 } else { 3 }, ::phenix_sdk::Value::Null));
        }});
    }
    let start =
        start.map(|(name, arguments)| quote!(::phenix_sdk::lifecycle(|| #name(#arguments))?;));
    let stop = stop
        .map(|(name, arguments)| quote!({ let state = &mut instance.state; ::phenix_sdk::lifecycle(|| #name(#arguments)) }))
        .unwrap_or_else(|| quote!(Ok::<(), ::phenix_sdk::Failure>(())));
    let generated = quote! {
        #[allow(clippy::large_const_arrays)]
        pub const METADATA: ::phenix_sdk::MetadataBytes = ::phenix_sdk::descriptor::static_metadata::metadata(#id, #version, ::phenix_sdk::MetadataExecution::StatePending, [#(#definitions),*], [#(#requirements),*], [#(#exports),*], [#(#properties),*]);
        const _: () = ::phenix_sdk::descriptor::static_metadata::validate_handlers([#(#provided),*], [#(#handlers),*]);
        #[used] #[no_mangle] #[link_section = ".phenix"]
        pub static PHENIX_DESCRIPTOR: [u8; ::phenix_sdk::NATIVE_SECTION_BYTES] = METADATA.native_section();
        pub mod imports { #[allow(unused_imports)] use super::*; #(#imports)* }
        pub mod properties { #[allow(unused_imports)] use super::*; #(#property_symbols)* }
        struct __Instance { state: State, initialized: bool, poisoned: bool }
        fn __failure(error: ::phenix_sdk::Failure) -> ::phenix_sdk::abi::Reply { ::phenix_sdk::abi::Reply::state_value(2, error.to_value()) }
        fn __invalid(message: &str) -> ::phenix_sdk::Failure { ::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed, message) }
        unsafe extern "C" fn __create(out: *mut *mut ::std::ffi::c_void) -> ::phenix_sdk::abi::Reply {
            ::phenix_sdk::live::boundary(|| {
                if out.is_null() { return __failure(__invalid("null instance output")); }
                unsafe { *out = ::std::ptr::null_mut(); }
                let state = State::default();
                unsafe { *out = Box::into_raw(Box::new(__Instance { state, initialized: false, poisoned: false })).cast(); }
                ::phenix_sdk::abi::Reply::state_value(0, ::phenix_sdk::Value::Null)
            })
        }
        unsafe extern "C" fn __initialize(ptr: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::StateHostApi) -> ::phenix_sdk::abi::Reply {
            ::phenix_sdk::live::boundary(|| {
                let result = (|| -> Result<(), ::phenix_sdk::Failure> {
                    if ptr.is_null() { return Err(__invalid("null initialization instance")); }
                    let api = unsafe { ::phenix_sdk::abi::StateHostApi::negotiate(api.cast()) }?;
                    #[allow(unused_variables)] let host = unsafe { ::phenix_sdk::live::Host::from_api(&api) };
                    let instance = unsafe { &mut *ptr.cast::<__Instance>() };
                    if instance.initialized || instance.poisoned { return Err(__invalid("initialization is single-use")); }
                    instance.initialized = true;
                    instance.poisoned = true;
                            #[allow(unused_variables)] let state = &mut instance.state;
                    #(#bindings)* #start
                    instance.poisoned = false;
                    Ok(())
                })();
                match result { Ok(()) => ::phenix_sdk::abi::Reply::state_value(0, ::phenix_sdk::Value::Null), Err(error) => __failure(error) }
            })
        }
        unsafe fn __entry(ptr: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::StateHostApi, key: ::phenix_sdk::abi::Bytes, input: Option<::phenix_sdk::abi::Bytes>, mode: u8) -> ::phenix_sdk::abi::Reply {
            ::phenix_sdk::live::boundary(|| {
                let result = (|| -> Result<::phenix_sdk::abi::Reply, ::phenix_sdk::Failure> {
                    if ptr.is_null() { return Err(__invalid("null invocation instance")); }
                    let api = unsafe { ::phenix_sdk::abi::StateHostApi::negotiate(api.cast()) }?;
                    #[allow(unused_variables)] let host = unsafe { ::phenix_sdk::live::Host::from_api(&api) };
                    let instance = unsafe { &mut *ptr.cast::<__Instance>() };
                    if !instance.initialized || (instance.poisoned && mode == 0) {
                        return Ok(::phenix_sdk::abi::Reply::state_value(5, __invalid("instance uninitialized or poisoned").to_value()));
                    }
                    let poisoned = instance.poisoned;
                    instance.poisoned = true;
                    let result = (|| -> Result<::phenix_sdk::abi::Reply, ::phenix_sdk::Failure> {
                        #[allow(unused_variables)] let state = &mut instance.state;
                        let key = ::std::str::from_utf8(unsafe { key.read() }?).map_err(|_| __invalid("invalid operation key"))?;
                        match mode {
                            0 => { #[allow(unused_variables)] let value = ::phenix_sdk::Value::decode_profile(unsafe { input.ok_or_else(|| __invalid("missing invocation input"))?.read() }?, ::phenix_sdk::WireProfile::V2)?; #(#invoke)* }
                            1 => { #(#poll)* }
                            _ => { #(#cancel)* }
                        }
                        Err(__invalid("operation absent from generated exports"))
                    })();
                    instance.poisoned = poisoned;
                    result
                })();
                match result { Ok(reply) => reply, Err(error) => __failure(error) }
            })
        }
        unsafe extern "C" fn __invoke(ptr: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::StateHostApi, key: ::phenix_sdk::abi::Bytes, input: ::phenix_sdk::abi::Bytes) -> ::phenix_sdk::abi::Reply { unsafe { __entry(ptr, api, key, Some(input), 0) } }
        unsafe extern "C" fn __poll(ptr: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::StateHostApi, key: ::phenix_sdk::abi::Bytes) -> ::phenix_sdk::abi::Reply { unsafe { __entry(ptr, api, key, None, 1) } }
        unsafe extern "C" fn __cancel(ptr: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::StateHostApi, key: ::phenix_sdk::abi::Bytes) -> ::phenix_sdk::abi::Reply { unsafe { __entry(ptr, api, key, None, 2) } }
        unsafe extern "C" fn __stop(ptr: *mut ::std::ffi::c_void) -> ::phenix_sdk::abi::Reply {
            ::phenix_sdk::live::boundary(|| {
                if ptr.is_null() { return __failure(__invalid("null stop instance")); }
                let mut instance = unsafe { Box::from_raw(ptr.cast::<__Instance>()) };
                let result = #stop;
                let cleanup = ::phenix_sdk::lifecycle(|| { drop(instance); Ok(()) });
                match result.and(cleanup) { Ok(()) => ::phenix_sdk::abi::Reply::state_value(0, ::phenix_sdk::Value::Null), Err(error) => __failure(error) }
            })
        }
        static __TABLE: ::phenix_sdk::abi::StatePluginTable = ::phenix_sdk::abi::StatePluginTable {
            header: ::phenix_sdk::abi::Header { major: 3, minor: 0, features: 3, table_bytes: ::std::mem::size_of::<::phenix_sdk::abi::StatePluginTable>() as u32 },
            create: Some(__create), initialize: Some(__initialize), invoke: Some(__invoke), poll: Some(__poll), cancel: Some(__cancel), stop: Some(__stop),
        };
        #[no_mangle] pub extern "C" fn phenix_native_v3() -> *const ::phenix_sdk::abi::Header { &__TABLE.header }
    };
    let extra: syn::File = syn::parse2(generated)?;
    items.extend(extra.items);
    Ok(quote!(#module))
}
