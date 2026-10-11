use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, spanned::Spanned, Data, DeriveInput, Expr, Fields, ItemMod, ItemType, Meta,
    Type,
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
#[proc_macro_derive(Portable)]
pub fn portable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = (|| {
        if !input.generics.params.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "portable generics are outside M0.0",
            ));
        }
        let name = &input.ident;
        let (schema, to, from) = match &input.data {
            Data::Struct(s) => {
                let Fields::Named(fields) = &s.fields else {
                    return Err(syn::Error::new(
                        s.fields.span(),
                        "use named portable fields",
                    ));
                };
                let mut fields: Vec<_> = fields.named.iter().collect();
                fields.sort_by_key(|f| f.ident.as_ref().unwrap().to_string());
                let names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
                let keys: Vec<_> = names.iter().map(|n| n.to_string()).collect();
                let types: Vec<_> = fields.iter().map(|f| &f.ty).collect();
                (
                    quote!(::phenix_sdk::Schema::Struct(&[#(::phenix_sdk::Field{name:#keys,schema:<#types as ::phenix_sdk::Portable>::SCHEMA}),*])),
                    quote!(::phenix_sdk::Value::map([#((#keys,::phenix_sdk::Portable::to_value(&self.#names))),*])),
                    quote! {let ::phenix_sdk::Value::Map(mut fields)=value else{return Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"expected struct"));};
                    let result=Self{#(#names:<#types as ::phenix_sdk::Portable>::from_value(fields.remove(#keys).ok_or_else(||::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,concat!("missing field ",#keys)))?)?),*};
                    if !fields.is_empty(){return Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"unknown field"));}Ok(result)},
                )
            }
            Data::Enum(e) => {
                if e.variants.iter().any(|v| !matches!(v.fields, Fields::Unit)) {
                    return Err(syn::Error::new(
                        e.enum_token.span,
                        "M0.0 supports unit error variants",
                    ));
                }
                let mut variants: Vec<_> = e.variants.iter().map(|v| &v.ident).collect();
                variants.sort_by_key(|v| v.to_string());
                let keys: Vec<_> = variants.iter().map(|v| v.to_string()).collect();
                (
                    quote!(::phenix_sdk::Schema::Variants(&[#(#keys),*])),
                    quote!(::phenix_sdk::Value::String(match self{#(Self::#variants=>#keys),*}.into())),
                    quote! {let ::phenix_sdk::Value::String(v)=value else{return Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"expected variant"));};match v.as_str(){#(#keys=>Ok(Self::#variants),)*_=>Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"unknown variant"))}},
                )
            }
            _ => return Err(syn::Error::new(input.span(), "unsupported portable type")),
        };
        Ok(
            quote! {impl ::phenix_sdk::Portable for #name{const SCHEMA: ::phenix_sdk::Schema=#schema;fn to_value(&self)->::phenix_sdk::Value{#to}fn from_value(value: ::phenix_sdk::Value)->Result<Self,::phenix_sdk::Failure>{#from}}},
        )
    })();
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}
#[proc_macro_attribute]
pub fn operation(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as ItemType);
    let result = (|| {
        let mut args = args(attr.into())?;
        let interface = required(&mut args, "interface")?;
        let operation = required(&mut args, "operation")?;
        no_extra(&args)?;
        let key = format!("{interface}/{operation}");
        if !phenix_contract::operation_key(&key) {
            return Err(syn::Error::new(item.span(), "invalid operation identity"));
        }
        let Type::Tuple(types) = item.ty.as_ref() else {
            return Err(syn::Error::new(
                item.span(),
                "operation type is (Request, Response, Error)",
            ));
        };
        if types.elems.len() != 3 {
            return Err(syn::Error::new(
                types.span(),
                "operation requires three types",
            ));
        }
        let r = &types.elems[0];
        let s = &types.elems[1];
        let e = &types.elems[2];
        let name = &item.ident;
        let vis = &item.vis;
        Ok(
            quote! {#vis struct #name;impl ::phenix_sdk::Operation for #name{type Request=#r;type Response=#s;type Error=#e;const DEFINITION: ::phenix_sdk::OperationDef=::phenix_sdk::OperationDef{key:#key,request:<#r as ::phenix_sdk::Portable>::SCHEMA,response:<#s as ::phenix_sdk::Portable>::SCHEMA,error:<#e as ::phenix_sdk::Portable>::SCHEMA};}},
        )
    })();
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}
#[proc_macro_attribute]
pub fn plugin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut module = parse_macro_input!(item as ItemMod);
    let result = (|| {
        let mut attrs = args(attr.into())?;
        let id = required(&mut attrs, "id")?;
        let version = required(&mut attrs, "version")?;
        no_extra(&attrs)?;
        if id == "root" || !phenix_contract::identifier(&id) {
            return Err(syn::Error::new(module.span(), "invalid plugin ID"));
        }
        let (_, items) = module.content.as_mut().ok_or_else(|| {
            syn::Error::new(module.ident.span(), "plugin requires an inline module")
        })?;
        let mut imports = Vec::new();
        let mut import_policies = Vec::new();
        let mut exports = Vec::new();
        let mut dispatch = Vec::new();
        let mut pending = Vec::new();
        let mut polls = Vec::new();
        let mut cancels = Vec::new();
        let mut state = None;
        let mut start_hook = None;
        let mut stop_hook = None;
        for item in items.iter_mut() {
            match item {
                syn::Item::Struct(s) if s.ident == "State" => {
                    state = Some(s.ident.clone());
                }
                syn::Item::Type(t) => {
                    let mut keep = Vec::new();
                    for a in t.attrs.drain(..) {
                        if a.path().is_ident("import") {
                            let mut policy = match &a.meta {
                                Meta::Path(_) => std::collections::BTreeMap::new(),
                                Meta::List(list) => args(list.tokens.clone())?,
                                _ => {
                                    return Err(syn::Error::new(a.span(), "expected import policy"))
                                }
                            };
                            let optional = match policy.remove("optional").as_deref() {
                                None | Some("false") => false,
                                Some("true") => true,
                                _ => {
                                    return Err(syn::Error::new(
                                        a.span(),
                                        "optional must be true or false",
                                    ))
                                }
                            };
                            let implementation = match policy.remove("implementation") {
                                Some(id) if phenix_contract::identifier(&id) && id != "root" => {
                                    quote!(Some(#id))
                                }
                                Some(_) => {
                                    return Err(syn::Error::new(a.span(), "invalid implementation"))
                                }
                                None => quote!(None),
                            };
                            no_extra(&policy)?;
                            import_policies.push(quote!(::phenix_sdk::ImportPolicy { optional:#optional, implementation:#implementation }));
                            let op = t.ty.as_ref();
                            let name = &t.ident;
                            imports.push(quote!(::phenix_sdk::Declaration{operation:<#op as ::phenix_sdk::Operation>::DEFINITION,provenance:concat!(module_path!(),"::",stringify!(#name))}));
                        } else {
                            keep.push(a);
                        }
                    }
                    t.attrs = keep;
                }
                syn::Item::Fn(f) => {
                    let mut keep = Vec::new();
                    for a in f.attrs.drain(..) {
                        if a.path().is_ident("start") || a.path().is_ident("stop") {
                            if !matches!(a.meta, Meta::Path(_)) || f.sig.asyncness.is_some() {
                                return Err(syn::Error::new(a.span(), "lifecycle hook must be synchronous without annotation arguments"));
                            }
                            let hook = if a.path().is_ident("start") {
                                &mut start_hook
                            } else {
                                &mut stop_hook
                            };
                            if hook.replace(f.sig.ident.clone()).is_some() {
                                return Err(syn::Error::new(a.span(), "duplicate lifecycle hook"));
                            }
                        } else if ["pending", "poll", "cancel"]
                            .iter()
                            .any(|name| a.path().is_ident(name))
                        {
                            let op: Type = a.parse_args()?;
                            if f.sig.asyncness.is_some() {
                                return Err(syn::Error::new(
                                    f.sig.span(),
                                    "pending hooks must be synchronous",
                                ));
                            }
                            let name = f.sig.ident.clone();
                            if a.path().is_ident("pending") {
                                exports.push(quote!(::phenix_sdk::Declaration{operation:<#op as ::phenix_sdk::Operation>::DEFINITION,provenance:concat!(module_path!(),"::",stringify!(#name))}));
                                pending.push((op, name));
                            } else if a.path().is_ident("poll") {
                                polls.push((op, name));
                            } else {
                                cancels.push((op, name));
                            }
                        } else if a.path().is_ident("export") {
                            let op: Type = a.parse_args()?;
                            if f.sig.asyncness.is_some() {
                                return Err(syn::Error::new(
                                    f.sig.span(),
                                    "async exports are deferred",
                                ));
                            }
                            let name = &f.sig.ident;
                            exports.push(quote!(::phenix_sdk::Declaration{operation:<#op as ::phenix_sdk::Operation>::DEFINITION,provenance:concat!(module_path!(),"::",stringify!(#name))}));
                            dispatch.push(quote!{if key==<#op as ::phenix_sdk::Operation>::DEFINITION.key{
                    let request=<<#op as ::phenix_sdk::Operation>::Request as ::phenix_sdk::Portable>::from_value(value)?;
                    let result: ::phenix_sdk::CallResult<<#op as ::phenix_sdk::Operation>::Response,< #op as ::phenix_sdk::Operation>::Error> = #name(state,&host,request);
                    return Ok(::phenix_sdk::reply(result));
                }});
                        } else {
                            keep.push(a);
                        }
                    }
                    f.attrs = keep;
                }
                _ => {}
            }
        }
        let state = state.ok_or_else(|| {
            syn::Error::new(module.ident.span(), "plugin needs a Default State struct")
        })?;
        let ni = imports.len();
        let ne = exports.len();
        if ne == 0 {
            return Err(syn::Error::new(
                module.ident.span(),
                "plugin must export an operation",
            ));
        }
        let is_pending = !pending.is_empty();
        if is_pending && !dispatch.is_empty() {
            return Err(syn::Error::new(
                module.ident.span(),
                "use one native execution profile per plugin",
            ));
        }
        if pending.len() != polls.len() || pending.len() != cancels.len() {
            return Err(syn::Error::new(
                module.ident.span(),
                "each pending export needs one poll and cancel hook",
            ));
        }
        let mut pending_dispatch = Vec::new();
        let mut poll_dispatch = Vec::new();
        let mut cancel_dispatch = Vec::new();
        let type_key = |t: &Type| quote!(#t).to_string();
        for (op, start) in &pending {
            let matching = |hooks: &Vec<(Type, syn::Ident)>| -> syn::Result<syn::Ident> {
                let names: Vec<_> = hooks
                    .iter()
                    .filter(|(t, _)| type_key(t) == type_key(op))
                    .collect();
                if names.len() != 1 {
                    return Err(syn::Error::new(
                        op.span(),
                        "pending export requires unique matching hook",
                    ));
                }
                Ok(names[0].1.clone())
            };
            let poll = matching(&polls)?;
            let cancel = matching(&cancels)?;
            pending_dispatch.push(quote! { if key == <#op as ::phenix_sdk::Operation>::DEFINITION.key {
                let request = <<#op as ::phenix_sdk::Operation>::Request as ::phenix_sdk::Portable>::from_value(value)?;
                let result: ::phenix_sdk::PendingResult<<#op as ::phenix_sdk::Operation>::Response,<#op as ::phenix_sdk::Operation>::Error> = #start(state, &host, request);
                return Ok(::phenix_sdk::pending_reply(result));
            }});
            poll_dispatch.push(quote! { if key == <#op as ::phenix_sdk::Operation>::DEFINITION.key {
                let result: ::phenix_sdk::PendingResult<<#op as ::phenix_sdk::Operation>::Response,<#op as ::phenix_sdk::Operation>::Error> = #poll(state, &host);
                return Ok(::phenix_sdk::pending_reply(result));
            }});
            cancel_dispatch.push(quote! { if key == <#op as ::phenix_sdk::Operation>::DEFINITION.key {
                let result: Result<bool, ::phenix_sdk::Failure> = #cancel(state, &host);
                return Ok(match result { Ok(true) => ::phenix_sdk::abi::Reply::value(4, ::phenix_sdk::Value::Null), Ok(false) => ::phenix_sdk::abi::Reply::value(3, ::phenix_sdk::Value::Null), Err(failure) => ::phenix_sdk::abi::Reply::failure(failure) });
            }});
        }
        let descriptor_fn = if is_pending {
            quote!(::phenix_sdk::descriptor_pending)
        } else {
            quote!(::phenix_sdk::descriptor_with_policy)
        };
        let descriptor = format_ident!("PHENIX_DESCRIPTOR");
        let mutable_state = if start_hook.is_some() || stop_hook.is_some() {
            quote!(mut)
        } else {
            quote!()
        };
        let mutable_instance = if stop_hook.is_some() {
            quote!(mut)
        } else {
            quote!()
        };
        let start = match start_hook {
            Some(name) => quote!(::phenix_sdk::lifecycle(|| #name(&mut state))),
            None => quote!(Ok::<(), ::phenix_sdk::Failure>(())),
        };
        let cleanup = match &stop_hook {
            Some(name) => quote!(::phenix_sdk::lifecycle(|| #name(&mut state))),
            None => quote!(Ok::<(), ::phenix_sdk::Failure>(())),
        };
        let stop = match stop_hook {
            Some(name) => quote!(::phenix_sdk::lifecycle(|| #name(&mut instance.state))),
            None => quote!(Ok::<(), ::phenix_sdk::Failure>(())),
        };
        let generated = quote! {
            #[used] #[no_mangle] #[link_section=".phenix"] pub static #descriptor:[u8;::phenix_sdk::SECTION_BYTES]=#descriptor_fn(#id,#version,[#(#imports),*],[#(#import_policies),*],[#(#exports),*]);
            const _:usize=#ni+#ne;
            struct __Instance { state: #state, poisoned: bool }
            unsafe extern "C" fn __create(out:*mut *mut ::std::ffi::c_void)->::phenix_sdk::abi::Reply{
                ::phenix_sdk::abi::boundary(||{if out.is_null(){return ::phenix_sdk::abi::Reply::failure(::phenix_sdk::Failure::new(::phenix_sdk::Code::AbiIncompatible,"null instance output"));}
                unsafe{*out=::std::ptr::null_mut();}
                let #mutable_state state=#state::default();
                if let Err(failure)=#start {let _=#cleanup;let _=::phenix_sdk::lifecycle(||{drop(state);Ok(())});return ::phenix_sdk::abi::Reply::failure(failure);}
                unsafe{*out=::std::boxed::Box::into_raw(::std::boxed::Box::new(__Instance{state,poisoned:false})).cast();}::phenix_sdk::abi::Reply::value(0,::phenix_sdk::Value::Null)})
            }
            unsafe extern "C" fn __invoke(instance:*mut ::std::ffi::c_void,api:*const ::phenix_sdk::abi::HostApi,key: ::phenix_sdk::abi::Bytes,input: ::phenix_sdk::abi::Bytes)->::phenix_sdk::abi::Reply{
                ::phenix_sdk::abi::boundary(||{
                    if instance.is_null()||api.is_null(){return ::phenix_sdk::abi::Reply::failure(::phenix_sdk::Failure::new(::phenix_sdk::Code::AbiIncompatible,"null invocation pointer"));}
                    let instance=unsafe{&mut *instance.cast::<__Instance>()};
                    if instance.poisoned{return ::phenix_sdk::abi::Reply::failure(::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed,"instance rejected after an earlier panic"));}
                    instance.poisoned=true;
                    let result=(||->Result<::phenix_sdk::abi::Reply,::phenix_sdk::Failure>{
                        let state=&mut instance.state;let host=unsafe{::phenix_sdk::Host::from_api(&*api)};
                        let key=::std::str::from_utf8(unsafe{key.read()}?).map_err(|_|::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected,"invalid operation key"))?;
                        let value=::phenix_sdk::Value::decode(unsafe{input.read()}?)?;#(#dispatch)*
                        Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed,"operation absent from generated exports"))
                    })();instance.poisoned=false;match result{Ok(reply)=>reply,Err(e)=>::phenix_sdk::abi::Reply::failure(e)}
                })
            }
            unsafe extern "C" fn __stop(instance:*mut ::std::ffi::c_void)->::phenix_sdk::abi::Reply{
                ::phenix_sdk::abi::boundary(||{if instance.is_null(){return ::phenix_sdk::abi::Reply::failure(::phenix_sdk::Failure::new(::phenix_sdk::Code::AbiIncompatible,"null stop pointer"));}
                let #mutable_instance instance=unsafe{::std::boxed::Box::from_raw(instance.cast::<__Instance>())};
                let result=#stop;let cleanup=::phenix_sdk::lifecycle(||{drop(instance);Ok(())});match result.and(cleanup){Ok(())=>::phenix_sdk::abi::Reply::value(0,::phenix_sdk::Value::Null),Err(failure)=>::phenix_sdk::abi::Reply::failure(failure)}})
            }
            static __TABLE: ::phenix_sdk::abi::PluginTable=::phenix_sdk::abi::PluginTable{
                header: ::phenix_sdk::abi::Header{major:1,minor:0,table_bytes: ::std::mem::size_of::<::phenix_sdk::abi::PluginTable>()as u32,features:0},create:__create,invoke:__invoke,stop:__stop};
            #[no_mangle] pub extern "C" fn phenix_native_v1()->*const ::phenix_sdk::abi::Header{&__TABLE.header}
        };
        let mut file: syn::File = syn::parse2(generated)?;
        if is_pending {
            file.items.retain(|item| match item {
                syn::Item::Fn(f) => f.sig.ident != "__invoke" && f.sig.ident != "phenix_native_v1",
                syn::Item::Static(s) => s.ident != "__TABLE",
                _ => true,
            });
            let pending_glue = quote! {
                unsafe fn __pending_entry(instance: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::PendingHostApi, key: ::phenix_sdk::abi::Bytes, input: Option<::phenix_sdk::abi::Bytes>, mode: u8) -> ::phenix_sdk::abi::Reply {
                    ::phenix_sdk::abi::boundary(|| {
                        if instance.is_null() || api.is_null() { return ::phenix_sdk::abi::Reply::failure(::phenix_sdk::Failure::new(::phenix_sdk::Code::AbiIncompatible, "null pending pointer")); }
                        let instance = unsafe { &mut *instance.cast::<__Instance>() };
                        // A caught panic may leave worker handles in State. Cancellation
                        // and drain still need access to them; invoke remains poisoned.
                        if instance.poisoned && mode == 0 { return ::phenix_sdk::abi::Reply::failure(::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed, "pending instance poisoned")); }
                        let poisoned = instance.poisoned;
                        instance.poisoned = true;
                        let result = (|| -> Result<::phenix_sdk::abi::Reply, ::phenix_sdk::Failure> {
                            let state = &mut instance.state;
                            let host = unsafe { ::phenix_sdk::PendingHost::from_api(&*api) };
                            let key = ::std::str::from_utf8(unsafe { key.read() }?).map_err(|_| ::phenix_sdk::Failure::new(::phenix_sdk::Code::SchemaRejected, "invalid pending key"))?;
                            match mode {
                                0 => { let value = ::phenix_sdk::Value::decode(unsafe { input.unwrap().read() }?)?; #(#pending_dispatch)* }
                                1 => { #(#poll_dispatch)* }
                                _ => { #(#cancel_dispatch)* }
                            }
                            Err(::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed, "operation absent from pending exports"))
                        })();
                        instance.poisoned = poisoned;
                        match result { Ok(reply) => reply, Err(e) => ::phenix_sdk::abi::Reply::failure(e) }
                    })
                }
                unsafe extern "C" fn __invoke(instance: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::PendingHostApi, key: ::phenix_sdk::abi::Bytes, input: ::phenix_sdk::abi::Bytes) -> ::phenix_sdk::abi::Reply { unsafe { __pending_entry(instance, api, key, Some(input), 0) } }
                unsafe extern "C" fn __poll(instance: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::PendingHostApi, key: ::phenix_sdk::abi::Bytes) -> ::phenix_sdk::abi::Reply { unsafe { __pending_entry(instance, api, key, None, 1) } }
                unsafe extern "C" fn __cancel(instance: *mut ::std::ffi::c_void, api: *const ::phenix_sdk::abi::PendingHostApi, key: ::phenix_sdk::abi::Bytes) -> ::phenix_sdk::abi::Reply { unsafe { __pending_entry(instance, api, key, None, 2) } }
                static __TABLE: ::phenix_sdk::abi::PendingPluginTable = ::phenix_sdk::abi::PendingPluginTable {
                    header: ::phenix_sdk::abi::Header { major: 2, minor: 0, table_bytes: ::std::mem::size_of::<::phenix_sdk::abi::PendingPluginTable>() as u32, features: 1 }, create: __create, invoke: __invoke, stop: __stop, poll: __poll, cancel: __cancel
                };
                #[no_mangle] pub extern "C" fn phenix_native_v2() -> *const ::phenix_sdk::abi::Header { &__TABLE.header }
            };
            let extra: syn::File = syn::parse2(pending_glue)?;
            file.items.extend(extra.items);
        }
        items.extend(file.items);
        Ok(quote!(#module))
    })();
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Derive the explicitly versioned M1 portable trait; M0 stays unchanged.
#[proc_macro_derive(LivePortable)]
pub fn live_portable(input: TokenStream) -> TokenStream {
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
