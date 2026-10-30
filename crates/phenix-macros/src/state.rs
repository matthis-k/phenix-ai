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
// Rust's type checker validates injected arguments against the declaration.
// Only the roles have syntax rules; response conversion stays type directed.
fn arguments(
    signature: &syn::Signature,
    request_allowed: bool,
    host_allowed: bool,
    state_type: &syn::Ident,
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
                    Some(name) if state_type == name => ("state", quote!(state)),
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
    let mut graph: Option<syn::Path> = None;
    for meta in declarations(tokens)? {
        if let Meta::List(list) = &meta {
            if list.path.is_ident("graph") {
                if graph.is_some() {
                    return Err(syn::Error::new(list.span(), "duplicate graph contribution"));
                }
                graph = Some(syn::parse2(list.tokens.clone())?);
                continue;
            }
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
            let provider = options.remove("provider");
            if !require && provider.is_some() {
                return Err(syn::Error::new(
                    list.span(),
                    "provider identity belongs to require",
                ));
            }
            let provider = match provider {
                Some(id) => quote!(Some(#id)),
                None => quote!(None),
            };
            no_extra(&options)?;
            let symbol: syn::Ident = syn::parse_str(&slot).map_err(|_| {
                syn::Error::new(list.span(), "slot symbol must be a Rust identifier")
            })?;
            if !slots.insert((require, slot.clone())) {
                return Err(syn::Error::new(list.span(), "duplicate slot"));
            }
            if require {
                let mut import_api = api.clone();
                if api
                    .segments
                    .first()
                    .is_some_and(|segment| segment.ident == "super")
                {
                    import_api.segments.insert(0, syn::parse_quote!(super));
                } else if api
                    .segments
                    .first()
                    .is_some_and(|segment| segment.ident == "self")
                {
                    import_api.segments.first_mut().unwrap().ident = syn::parse_quote!(super);
                }
                let selector = if indexed {
                    format!("{slot}#0")
                } else {
                    slot.clone()
                };
                let invoke_at = if indexed {
                    quote! {
                        pub fn invoke_at<O: #import_api::Member>(host: &::phenix_sdk::live::Host<'_>, index: usize, request: &O::Request) -> ::phenix_sdk::CallResult<O::Response, O::Error> {
                            host.invoke::<O>(&format!("{}#{}", #slot, index), request)
                        }
                        pub fn begin_at<O: #import_api::Member>(host: &::phenix_sdk::live::Host<'_>, index: usize, request: &O::Request) -> Result<::phenix_sdk::ImportCall<O>, ::phenix_sdk::Failure> {
                            host.begin::<O>(&format!("{}#{}", #slot, index), request)
                        }
                        pub fn begin_scoped_at<O: #import_api::Member>(host: &::phenix_sdk::live::Host<'_>, index: usize, scope: &::phenix_sdk::ScopedRequest, request: &O::Request) -> Result<::phenix_sdk::ImportCall<O>, ::phenix_sdk::Failure> {
                            host.begin_scoped::<O>(&format!("{}#{}", #slot, index), scope, request)
                        }
                    }
                } else {
                    quote!()
                };
                requirements.push(quote!(::phenix_sdk::StaticImport { provider:#provider, slot: #slot, contract: #api::CONTRACT.contract, revision_min: #api::CONTRACT.revision, revision_max: #api::CONTRACT.revision, cardinality: #cardinality }));
                imports.push(quote! {
                    #[allow(non_camel_case_types)] pub struct #symbol;
                    impl #symbol {
                        pub fn invoke<O: #import_api::Member>(host: &::phenix_sdk::live::Host<'_>, request: &O::Request) -> ::phenix_sdk::CallResult<O::Response, O::Error> { host.invoke::<O>(#selector, request) }
                        pub fn begin<O: #import_api::Member>(host: &::phenix_sdk::live::Host<'_>, request: &O::Request) -> Result<::phenix_sdk::ImportCall<O>, ::phenix_sdk::Failure> { host.begin::<O>(#selector, request) }
                        pub fn begin_scoped<O: #import_api::Member>(host: &::phenix_sdk::live::Host<'_>, scope: &::phenix_sdk::ScopedRequest, request: &O::Request) -> Result<::phenix_sdk::ImportCall<O>, ::phenix_sdk::Failure> { host.begin_scoped::<O>(#selector, scope, request) }
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
    // Domain-first syntax: an annotated ordinary Rust struct and its inherent
    // methods are the source of plugin-owned state and exported operations.
    // Existing free handler declarations continue to work unchanged.
    let mut selected_state: Option<syn::Ident> = None;
    for item in items.iter_mut() {
        if let syn::Item::Struct(domain) = item {
            let mut marked = false;
            domain.attrs.retain(|attr| {
                if attr.path().is_ident("state") {
                    marked = true;
                    false
                } else {
                    true
                }
            });
            if marked {
                if selected_state.is_some() {
                    return Err(syn::Error::new(domain.span(), "duplicate #[state] owner"));
                }
                selected_state = Some(domain.ident.clone());
            }
        }
    }
    // Inherent #[provide] methods already identify their Rust state owner.
    // Inferring it avoids requiring domain structs to carry a Phenix marker.
    let mut inferred_state: Option<syn::Ident> = None;
    for item in items.iter() {
        let syn::Item::Impl(implementation) = item else {
            continue;
        };
        if !implementation.items.iter().any(|member| {
            matches!(member, syn::ImplItem::Fn(method)
                if method.attrs.iter().any(|attr| attr.path().is_ident("provide")))
        }) {
            continue;
        }
        let Type::Path(owner) = implementation.self_ty.as_ref() else {
            return Err(syn::Error::new(
                implementation.self_ty.span(),
                "exported methods require a local nominal state type",
            ));
        };
        let Some(name) = owner.path.get_ident() else {
            return Err(syn::Error::new(
                owner.span(),
                "exported methods require a simple local state type",
            ));
        };
        if let Some(previous) = &inferred_state {
            if previous != name {
                return Err(syn::Error::new(
                    name.span(),
                    "multiple exported state owners; use one domain owner or separate plugins",
                ));
            }
        } else {
            inferred_state = Some(name.clone());
        }
    }
    if let (Some(explicit), Some(inferred)) = (&selected_state, &inferred_state) {
        if explicit != inferred {
            return Err(syn::Error::new(
                inferred.span(),
                "explicit state owner disagrees with exported methods",
            ));
        }
    }
    let state_ident = selected_state
        .clone()
        .or(inferred_state)
        .unwrap_or_else(|| syn::Ident::new("State", module.ident.span()));
    if selected_state.is_some()
        && items.iter().any(|item| {
            matches!(item, syn::Item::Struct(domain) if domain.ident == "State"
                && domain.ident != state_ident)
        })
    {
        return Err(syn::Error::new(
            module.ident.span(),
            "annotated state cannot coexist with legacy State",
        ));
    }
    let mut generated_handlers = Vec::new();
    for item in items.iter_mut() {
        let syn::Item::Impl(implementation) = item else {
            continue;
        };
        for member in &mut implementation.items {
            let syn::ImplItem::Fn(method) = member else {
                continue;
            };
            let positions: Vec<_> = method
                .attrs
                .iter()
                .enumerate()
                .filter(|(_, attribute)| attribute.path().is_ident("provide"))
                .map(|(index, _)| index)
                .collect();
            if positions.is_empty() {
                continue;
            }
            if positions.len() != 1
                || implementation.trait_.is_some()
                || !implementation.generics.params.is_empty()
                || implementation.generics.where_clause.is_some()
                || !matches!(
                    implementation.self_ty.as_ref(),
                    Type::Path(path) if path.qself.is_none()
                        && path.path.is_ident(&state_ident)
                )
                || method.sig.asyncness.is_some()
                || method.sig.unsafety.is_some()
                || method.sig.constness.is_some()
                || method.sig.abi.is_some()
                || !method.sig.generics.params.is_empty()
                || method.sig.generics.where_clause.is_some()
            {
                return Err(syn::Error::new(
                    method.span(),
                    "#[provide] method needs a synchronous inherent impl on the plugin's state type",
                ));
            }
            let attr = method.attrs.remove(positions[0]);
            let operation: syn::Type = attr.parse_args()?;
            let mut inputs = method.sig.inputs.iter();
            let Some(syn::FnArg::Receiver(receiver)) = inputs.next() else {
                return Err(syn::Error::new(
                    method.sig.span(),
                    "exported method needs borrowed self",
                ));
            };
            if receiver.reference.is_none() || receiver.colon_token.is_some() {
                return Err(syn::Error::new(
                    receiver.span(),
                    "exported method needs &self or &mut self",
                ));
            }
            let (request, argument) = match (inputs.next(), inputs.next()) {
                (None, None) => (quote!(), quote!()),
                (Some(syn::FnArg::Typed(param)), None) => {
                    let ty = &param.ty;
                    (quote!(, request: #ty), quote!(request))
                }
                _ => {
                    return Err(syn::Error::new(
                        method.sig.span(),
                        "exported method needs at most one typed request",
                    ))
                }
            };
            let output = &method.sig.output;
            let name = &method.sig.ident;
            let generated = syn::Ident::new(
                &format!("__phenix_export_{}", generated_handlers.len()),
                name.span(),
            );
            generated_handlers.push(syn::parse2::<syn::ItemFn>(quote! {
                #[provide(#operation)]
                fn #generated(state: &mut #state_ident #request) #output {
                    state.#name(#argument)
                }
            })?);
        }
    }
    items.extend(generated_handlers.into_iter().map(syn::Item::Fn));
    let mut state = false;
    let mut properties = Vec::new();
    let mut property_symbols = Vec::new();
    let mut bindings = Vec::new();
    let mut owned_fields = Vec::new();
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
            syn::Item::Struct(s) if s.ident == state_ident => {
                if state
                    || !s.generics.params.is_empty()
                    || !matches!(s.fields, syn::Fields::Named(_) | syn::Fields::Unit)
                {
                    return Err(syn::Error::new(
                        s.span(),
                        "State must be a unique nongeneric named struct",
                    ));
                }
                state = true;
                let mut sidecars = Vec::new();
                for field in &mut s.fields {
                    let mut access = [false; 3];
                    let mut exposed = false;
                    let mut stable = None;
                    let mut keep = Vec::new();
                    for attr in field.attrs.drain(..) {
                        if attr.path().is_ident("expose") {
                            if exposed || !matches!(attr.meta, Meta::Path(_)) {
                                return Err(syn::Error::new(
                                    attr.span(),
                                    "expose must occur once without arguments",
                                ));
                            }
                            exposed = true;
                            continue;
                        }
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
                    if exposed || access.iter().any(|v| *v) {
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
                        if exposed && access[1] {
                            return Err(syn::Error::new(
                                field.span(),
                                "exposed Rust state is owner-published; external set needs an owner handler",
                            ));
                        }
                        let access = if exposed {
                            [true, false, access[2]]
                        } else {
                            access
                        };
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
                        if exposed {
                            // A sidecar keeps the owner-only property reference
                            // and its last committed revision. The actual T
                            // remains a plain Rust field in State.
                            let sidecar =
                                syn::Ident::new(&format!("__published_{}", name), name.span());
                            let method = syn::Ident::new(&format!("publish_{}", name), name.span());
                            let update_method =
                                syn::Ident::new(&format!("update_{}", name), name.span());
                            let try_update_method =
                                syn::Ident::new(&format!("try_update_{}", name), name.span());
                            let reference_method =
                                syn::Ident::new(&format!("reference_{}", name), name.span());
                            sidecars.push(syn::Field::parse_named.parse2(quote!(
                                #sidecar: Option<(::phenix_sdk::live::ValueRef<#ty>, ::phenix_sdk::Revision)>
                            ))?);
                            bindings.push(quote! {
                                let reference = host.create(#stable, &state.#name).map_err(|error| match error {
                                    ::phenix_sdk::live::AccessError::Kernel(error) => error,
                                    ::phenix_sdk::live::AccessError::State(error) =>
                                        ::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed, format!("property initialization: {error:?}")),
                                })?;
                                state.#sidecar = Some((reference, ::phenix_sdk::Revision(0)));
                            });
                            owned_fields.push(quote! {
                                #[allow(dead_code)]
                                pub fn #reference_method(
                                    &self,
                                ) -> Result<::phenix_sdk::live::ValueRef<#ty>, ::phenix_sdk::Failure> {
                                    self.#sidecar
                                        .map(|(reference, _)| reference)
                                        .ok_or_else(|| ::phenix_sdk::Failure::new(
                                            ::phenix_sdk::Code::ScopeRejected,
                                            "owned property is not initialized",
                                        ))
                                }

                                #[allow(dead_code)]
                                pub fn #method(
                                    &mut self,
                                    host: &::phenix_sdk::live::Host<'_>,
                                    next: #ty,
                                ) -> ::phenix_sdk::live::AccessResult<::phenix_sdk::live::Snapshot<#ty>> {
                                    let (reference, revision) = self.#sidecar.ok_or_else(|| {
                                        ::phenix_sdk::Failure::new(::phenix_sdk::Code::ScopeRejected, "owned property is not initialized")
                                    })?;
                                    // Publish first, then commit Rust state. Failed
                                    // publication leaves the owner value unchanged.
                                    let published = host.publish(reference, revision, &next)?;
                                    self.#name = next;
                                    self.#sidecar = Some((reference, published.revision));
                                    Ok(published)
                                }

                                /// Construct, publish and commit a candidate without
                                /// mutating the authoritative field speculatively.
                                #[allow(dead_code)]
                                pub fn #update_method(
                                    &mut self,
                                    host: &::phenix_sdk::live::Host<'_>,
                                    change: impl FnOnce(&#ty) -> #ty,
                                ) -> ::phenix_sdk::live::AccessResult<::phenix_sdk::live::Snapshot<#ty>> {
                                    let candidate = change(&self.#name);
                                    self.#method(host, candidate)
                                }

                                /// Validate a proposed change before publication. The
                                /// domain rejection and CAS failure are distinct; neither
                                /// changes the plugin-owned field.
                                #[allow(dead_code)]
                                pub fn #try_update_method<E>(
                                    &mut self,
                                    host: &::phenix_sdk::live::Host<'_>,
                                    change: impl FnOnce(&#ty) -> Result<#ty, E>,
                                ) -> Result<::phenix_sdk::live::Snapshot<#ty>, ::phenix_sdk::live::OwnedUpdateError<E>> {
                                    let candidate = change(&self.#name)
                                        .map_err(::phenix_sdk::live::OwnedUpdateError::Rejected)?;
                                    self.#method(host, candidate)
                                        .map_err(::phenix_sdk::live::OwnedUpdateError::Publish)
                                }
                            });
                        } else {
                            bindings.push(quote!(state.#name.bind(&host, #stable).map_err(|error| match error { ::phenix_sdk::live::AccessError::Kernel(error) => error, ::phenix_sdk::live::AccessError::State(error) => ::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed, format!("property initialization: {error:?}")) })?;));
                            field.ty = syn::parse_quote!(::phenix_sdk::live::Cell<#ty>);
                        }
                    }
                }
                if !sidecars.is_empty() {
                    let syn::Fields::Named(named) = &mut s.fields else {
                        return Err(syn::Error::new(
                            s.span(),
                            "exposed state needs named fields",
                        ));
                    };
                    named.named.extend(sidecars);
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
                            arguments(&function.sig, request_allowed, !lifecycle, &state_ident)?;
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
            return Ok(match result {
                Err(::phenix_sdk::CallError::Kernel(error)) => ::phenix_sdk::abi::Reply::value(5, error.to_value()),
                result => ::phenix_sdk::live::reply(result),
            });
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
                return Ok(::phenix_sdk::abi::Reply::value(5, ::phenix_sdk::Failure::new(::phenix_sdk::Code::AbiIncompatible, "pending operation requires an existing native ticket").to_value()));
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
            return Ok(::phenix_sdk::abi::Reply::value(if result? { 4 } else { 3 }, ::phenix_sdk::Value::Null));
        }});
    }
    let start =
        start.map(|(name, arguments)| quote!(::phenix_sdk::lifecycle(|| #name(#arguments))?;));
    let stop = stop
        .map(|(name, arguments)| quote!({ let state = &mut instance.state; ::phenix_sdk::lifecycle(|| #name(#arguments)) }))
        .unwrap_or_else(|| quote!(Ok::<(), ::phenix_sdk::Failure>(())));
    let graph_descriptor = graph.map(|path| quote! {
        #[allow(clippy::large_const_arrays)]
        pub const GRAPH_METADATA: ::phenix_sdk::MetadataBytes =
            ::phenix_sdk::descriptor::static_metadata::graph(#path);
        #[used] #[no_mangle] #[link_section = ".phenix.graph"]
        pub static PHENIX_GRAPH: [u8; ::phenix_sdk::NATIVE_SECTION_BYTES] = GRAPH_METADATA.native_section();
    });
    let generated = quote! {
        impl #state_ident {
            #(#owned_fields)*
        }
        #graph_descriptor
        #[allow(clippy::large_const_arrays)]
        pub const METADATA: ::phenix_sdk::MetadataBytes = ::phenix_sdk::descriptor::static_metadata::metadata(#id, #version, ::phenix_sdk::MetadataExecution::StatePending, [#(#definitions),*], [#(#requirements),*], [#(#exports),*], [#(#properties),*]);
        const _: () = ::phenix_sdk::descriptor::static_metadata::validate_handlers([#(#provided),*], [#(#handlers),*]);
        #[used] #[no_mangle] #[link_section = ".phenix"]
        pub static PHENIX_DESCRIPTOR: [u8; ::phenix_sdk::NATIVE_SECTION_BYTES] = METADATA.native_section();
        pub mod imports { #[allow(unused_imports)] use super::*; #(#imports)* }
        pub mod properties { #[allow(unused_imports)] use super::*; #(#property_symbols)* }
        struct __Instance { state: #state_ident, initialized: bool, poisoned: bool }
        fn __failure(error: ::phenix_sdk::Failure) -> ::phenix_sdk::abi::Reply { ::phenix_sdk::abi::Reply::value(2, error.to_value()) }
        fn __invalid(message: &str) -> ::phenix_sdk::Failure { ::phenix_sdk::Failure::new(::phenix_sdk::Code::PluginFailed, message) }
        unsafe extern "C" fn __create(out: *mut *mut ::std::ffi::c_void) -> ::phenix_sdk::abi::Reply {
            ::phenix_sdk::live::boundary(|| {
                if out.is_null() { return __failure(__invalid("null instance output")); }
                unsafe { *out = ::std::ptr::null_mut(); }
                let state = #state_ident::default();
                unsafe { *out = Box::into_raw(Box::new(__Instance { state, initialized: false, poisoned: false })).cast(); }
                ::phenix_sdk::abi::Reply::value(0, ::phenix_sdk::Value::Null)
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
                match result { Ok(()) => ::phenix_sdk::abi::Reply::value(0, ::phenix_sdk::Value::Null), Err(error) => __failure(error) }
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
                        return Ok(::phenix_sdk::abi::Reply::value(5, __invalid("instance uninitialized or poisoned").to_value()));
                    }
                    let poisoned = instance.poisoned;
                    instance.poisoned = true;
                    let result = (|| -> Result<::phenix_sdk::abi::Reply, ::phenix_sdk::Failure> {
                        #[allow(unused_variables)] let state = &mut instance.state;
                        let key = ::std::str::from_utf8(unsafe { key.read() }?).map_err(|_| __invalid("invalid operation key"))?;
                        match mode {
                            0 => { #[allow(unused_variables)] let value = ::phenix_sdk::Value::decode(unsafe { input.ok_or_else(|| __invalid("missing invocation input"))?.read() }?)?; #(#invoke)* }
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
                match result.and(cleanup) { Ok(()) => ::phenix_sdk::abi::Reply::value(0, ::phenix_sdk::Value::Null), Err(error) => __failure(error) }
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

#[cfg(test)]
mod domain_method_tests {
    use super::*;
    use quote::quote;

    #[test]
    fn domain_methods_generate_abi_hooks_without_free_function_wrappers() {
        let module: ItemMod = syn::parse_quote! {
            mod plugin {
                #[derive(Default)]
                struct Counter { value: u64 }

                impl Counter {
                    #[provide(CounterApi::increase)]
                    fn increase(&mut self, change: u64) -> Result<u64, String> {
                        self.value += change;
                        Ok(self.value)
                    }

                    #[provide(CounterApi::read)]
                    fn read(&self) -> u64 { self.value }
                }
            }
        };
        let generated = expand(
            quote!(
                id = "test.counter",
                define(CounterApi),
                provide(CounterApi, slot = "Counter")
            ),
            module,
        )
        .unwrap()
        .to_string();
        assert!(generated.contains("struct Counter"));
        assert!(generated.contains("impl Counter"));
        assert!(generated.contains("__phenix_export_0"));
        assert!(generated.contains("__phenix_export_1"));
        assert!(generated.contains("state . increase (request)"));
        assert!(generated.contains("state . read ()"));
        assert!(generated.contains("let state = Counter :: default"));
        assert!(generated.contains("state : Counter"));
        assert!(!generated.contains("struct State"));
    }

    #[test]
    fn inferred_state_owner_needs_no_annotation() {
        let module: ItemMod = syn::parse_quote! {
            mod plugin {
                #[derive(Default)]
                struct Meter { total: u64 }
                impl Meter {
                    #[provide(MeterApi::add)]
                    fn add(&mut self, by: u64) -> u64 {
                        self.total += by;
                        self.total
                    }
                }
            }
        };
        let generated = expand(
            quote!(id = "test.meter", provide(MeterApi, slot = "Meter")),
            module,
        )
        .unwrap()
        .to_string();
        assert!(generated.contains("state : Meter"));
        assert!(generated.contains("let state = Meter :: default"));
        assert!(generated.contains("state . add (request)"));
        assert!(!generated.contains("struct State"));
    }

    #[test]
    fn distinct_inferred_owners_report_ambiguity() {
        let module: ItemMod = syn::parse_quote! {
            mod plugin {
                #[derive(Default)]
                struct Left;
                #[derive(Default)]
                struct Right;
                impl Left {
                    #[provide(Api::first)]
                    fn first(&self) -> u64 { 1 }
                }
                impl Right {
                    #[provide(Api::second)]
                    fn second(&self) -> u64 { 2 }
                }
            }
        };
        let error = expand(
            quote!(id = "test.domain", provide(Api, slot = "Api")),
            module,
        )
        .unwrap_err();
        assert!(error.to_string().contains("multiple exported state owners"));
    }

    #[test]
    fn inappropriate_inherent_method_exports_reject() {
        for module in [
            syn::parse_quote! {
                mod plugin {
                    #[derive(Default)]
                    #[state]
                    struct Counter;
                    struct Other;
                    impl Other {
                        #[provide(CounterApi::read)]
                        fn read(&self) -> u64 { 0 }
                    }
                }
            },
            syn::parse_quote! {
                mod plugin {
                    #[derive(Default)]
                    #[state]
                    struct Counter;
                    impl Counter {
                        #[provide(CounterApi::read)]
                        async fn read(&self) -> u64 { 0 }
                    }
                }
            },
            syn::parse_quote! {
                mod plugin {
                    #[derive(Default)]
                    #[state]
                    struct Counter;
                    impl Counter {
                        #[provide(CounterApi::read)]
                        fn read(self) -> u64 { 0 }
                    }
                }
            },
        ] {
            assert!(expand(
                quote!(id = "test.counter", provide(CounterApi, slot = "Counter")),
                module,
            )
            .is_err());
        }
    }

    #[test]
    fn duplicate_state_owner_rejects() {
        let module: ItemMod = syn::parse_quote! {
            mod plugin {
                #[derive(Default)]
                #[state]
                struct Left;
                #[derive(Default)]
                #[state]
                struct Right;
            }
        };
        assert!(expand(quote!(id = "test.counter"), module).is_err());
    }
}

#[cfg(test)]
mod owned_state_tests {
    use super::*;

    #[test]
    fn expose_preserves_plain_rust_state_and_omits_public_set() {
        let module: ItemMod = syn::parse_quote! {
            mod plugin {
                #[derive(Default)]
                struct State {
                    #[expose]
                    #[watch]
                    counter: u64,
                    private: u64,
                }
            }
        };
        let generated = expand(quote!(id = "test.owned"), module)
            .unwrap()
            .to_string();
        assert!(generated.contains("__published_counter"));
        assert!(generated.contains("publish_counter"));
        assert!(generated.contains("update_counter"));
        assert!(generated.contains("try_update_counter"));
        assert!(generated.contains("OwnedUpdateError"));
        assert!(generated.contains("Rejected"));
        assert!(generated.contains("Publish"));
        assert!(generated.contains("state . counter"));
        assert!(generated.contains("get : true"));
        assert!(generated.contains("set : false"));
        assert!(generated.contains("watch : true"));
        assert!(!generated.contains("state . counter . bind"));
    }

    #[test]
    fn exposed_fields_reject_public_set() {
        let module: ItemMod = syn::parse_quote! {
            mod plugin {
                #[derive(Default)]
                struct State {
                    #[expose]
                    #[set]
                    secret: u64,
                }
            }
        };
        let error = expand(quote!(id = "test.owned"), module).unwrap_err();
        assert!(error
            .to_string()
            .contains("external set needs an owner handler"));
    }
}
