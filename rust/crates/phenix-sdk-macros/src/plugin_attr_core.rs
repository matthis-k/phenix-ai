use crate::component_attr::{
    export_descriptor, export_error_type, export_signature_types, is_call_context_parameter,
    parse_export, value_response_type, ExportContribution,
};
use crate::interface_attr::validate_interface_id;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::Parser, parse_quote, punctuated::Punctuated, Attribute, Expr, ExprLit, Fields, FnArg,
    Ident, ImplItem, Item, ItemImpl, ItemMod, ItemStruct, Lit, LitStr, Meta, ReturnType, Token,
    Type,
};

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    if let Ok(item) = syn::parse2::<ItemStruct>(input.clone()) {
        return expand_struct(args, item);
    }
    if let Ok(item) = syn::parse2::<ItemMod>(input.clone()) {
        return expand_module(args, item);
    }
    if let Ok(item) = syn::parse2::<ItemImpl>(input.clone()) {
        return expand_lifecycle_impl(args, item);
    }

    Err(syn::Error::new_spanned(
        input,
        "#[phenix_sdk::plugin] applies to a plugin struct, stateless inline module, or plugin lifecycle impl",
    ))
}

fn expand_lifecycle_impl(args: TokenStream, mut item: ItemImpl) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "plugin lifecycle impls inherit plugin identity and do not accept root arguments",
        ));
    }
    if item.trait_.is_some() || !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item,
            "plugin lifecycle must be a non-generic inherent impl",
        ));
    }

    let mut start = None;
    let mut stop = None;
    for member in &mut item.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        let mut retained = Vec::new();
        let mut lifecycle = None;
        for attribute in std::mem::take(&mut method.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            if lifecycle.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "a lifecycle method may declare only one Phenix role",
                ));
            }
            lifecycle = Some(parse_lifecycle_role(&attribute)?);
        }
        method.attrs = retained;

        let Some(role) = lifecycle else {
            continue;
        };
        validate_lifecycle_signature(method)?;
        let slot = match role {
            LifecycleRole::Start => &mut start,
            LifecycleRole::Stop => &mut stop,
        };
        if slot.is_some() {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                format!("duplicate plugin {} lifecycle method", role.name()),
            ));
        }
        *slot = Some(method.sig.ident.clone());
    }

    let self_ty = &item.self_ty;
    let start = start
        .as_ref()
        .map(|method| quote!(Some(stringify!(#method))))
        .unwrap_or_else(|| quote!(None));
    let stop = stop
        .as_ref()
        .map(|method| quote!(Some(stringify!(#method))))
        .unwrap_or_else(|| quote!(None));

    Ok(quote! {
        #item

        impl ::phenix_sdk::StaticPluginLifecycle for #self_ty {
            fn lifecycle() -> ::phenix_sdk::StaticPluginLifecycleDescriptor {
                ::phenix_sdk::StaticPluginLifecycleDescriptor {
                    start: #start,
                    stop: #stop,
                }
            }
        }
    })
}

#[derive(Clone, Copy)]
enum LifecycleRole {
    Start,
    Stop,
}

impl LifecycleRole {
    fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }
}

fn parse_lifecycle_role(attribute: &Attribute) -> syn::Result<LifecycleRole> {
    let Meta::List(meta) = &attribute.meta else {
        return Err(syn::Error::new_spanned(
            attribute,
            "lifecycle attributes must use #[phenix(start)] or #[phenix(stop)]",
        ));
    };
    let roles = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    if roles.len() != 1 {
        return Err(syn::Error::new_spanned(
            attribute,
            "lifecycle attributes must contain exactly start or stop",
        ));
    }
    let Some(Meta::Path(role)) = roles.first() else {
        return Err(syn::Error::new_spanned(
            attribute,
            "lifecycle attributes must contain exactly start or stop",
        ));
    };
    if role.is_ident("start") {
        Ok(LifecycleRole::Start)
    } else if role.is_ident("stop") {
        Ok(LifecycleRole::Stop)
    } else {
        Err(syn::Error::new_spanned(
            role,
            "unsupported plugin lifecycle role",
        ))
    }
}

fn validate_lifecycle_signature(method: &syn::ImplItemFn) -> syn::Result<()> {
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            "plugin lifecycle methods cannot be generic",
        ));
    }

    if method.sig.asyncness.is_some() || method.sig.inputs.len() != 2 {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "plugin lifecycle methods must be synchronous fn(&mut self, context) -> Result<_, _>",
        ));
    }
    let Some(FnArg::Receiver(receiver)) = method.sig.inputs.first() else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "plugin lifecycle methods require &mut self",
        ));
    };
    if receiver.reference.is_none() || receiver.mutability.is_none() {
        return Err(syn::Error::new_spanned(
            receiver,
            "plugin lifecycle methods require &mut self",
        ));
    }
    let Some(FnArg::Typed(context)) = method.sig.inputs.iter().nth(1) else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "plugin lifecycle methods require &PluginContext",
        ));
    };
    let Type::Reference(context_reference) = context.ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            &context.ty,
            "plugin lifecycle methods require &PluginContext",
        ));
    };
    if context_reference.mutability.is_some() {
        return Err(syn::Error::new_spanned(
            &context.ty,
            "plugin lifecycle methods require shared &PluginContext",
        ));
    }
    let Type::Path(context_type) = context_reference.elem.as_ref() else {
        return Err(syn::Error::new_spanned(
            &context.ty,
            "plugin lifecycle methods require &PluginContext",
        ));
    };
    if context_type
        .path
        .segments
        .last()
        .is_none_or(|segment| segment.ident != "PluginContext")
    {
        return Err(syn::Error::new_spanned(
            &context.ty,
            "plugin lifecycle methods require &PluginContext",
        ));
    }
    let ReturnType::Type(_, output) = &method.sig.output else {
        return Err(syn::Error::new_spanned(
            &method.sig.output,
            "plugin lifecycle methods must return Result",
        ));
    };
    let Type::Path(output) = output.as_ref() else {
        return Err(syn::Error::new_spanned(
            output,
            "plugin lifecycle methods must return Result",
        ));
    };
    if output
        .path
        .segments
        .last()
        .is_none_or(|segment| segment.ident != "Result")
    {
        return Err(syn::Error::new_spanned(
            output,
            "plugin lifecycle methods must return Result",
        ));
    }
    Ok(())
}

fn expand_struct(args: TokenStream, mut item: ItemStruct) -> syn::Result<TokenStream> {
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "Phenix plugins must have one concrete runtime identity",
        ));
    }

    let execution = plugin_execution(args.clone())?;
    let authority = plugin_authority(args.clone())?;
    let version = plugin_version(args.clone())?;
    let contributions = field_contributions(&mut item)?;
    let has_root_component = !contributions.imports.is_empty()
        || !contributions.hosts.is_empty()
        || !contributions.events.is_empty();
    if !contributions.components.is_empty() || has_root_component {
        if plugin_execution_is_resource_only(args.clone())? {
            return Err(syn::Error::new_spanned(
                &item.ident,
                "resource-only plugins cannot declare embedded component fields",
            ));
        }
        if plugin_execution_is_runtime_hosted(args.clone())? {
            return Err(syn::Error::new_spanned(
                &item.ident,
                "runtime-hosted plugins cannot declare embedded component fields",
            ));
        }
    }
    validate_nested_ids(
        &resolve_plugin_id(args.clone(), &item.ident)?,
        &contributions,
    )?;
    let dependency_types = contributions
        .dependencies
        .iter()
        .map(|dependency| &dependency.ty);
    let plugin_name = &item.ident;
    let dependency_aliases = contributions.dependencies.iter().map(|dependency| {
        let field = &dependency.field;
        let ty = &dependency.ty;
        let alias = Ident::new(
            &format!("__PhenixDependency_{plugin_name}_{field}"),
            field.span(),
        );
        quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            type #alias = #ty;
        }
    });
    let dependency_modules = contributions.dependencies.iter().map(|dependency| {
        let field = &dependency.field;
        let alias = Ident::new(
            &format!("__PhenixDependency_{plugin_name}_{field}"),
            field.span(),
        );
        quote! {
            pub mod #field {
                pub type Plugin = super::super::super::#alias;
            }
        }
    });
    let dependency_access = if item.ident == "Plugin" {
        quote! {
            pub mod plugin {
                pub mod dependencies {
                    #(#dependency_modules)*
                }
            }
        }
    } else {
        TokenStream::new()
    };
    let mut component_descriptors = contributions
        .components
        .iter()
        .map(|component| {
            let field = &component.field;
            let ty = &component.ty;
            if let Some(id) = &component.id {
                quote! {
                    ::phenix_sdk::StaticComponentDescriptor::explicit::<#ty>(
                        #id,
                        stringify!(#field),
                    )
                }
            } else {
                quote! {
                    ::phenix_sdk::StaticComponentDescriptor::derived::<#ty>(
                        &Self::plugin_id(),
                        stringify!(#field),
                    )
                }
            }
        })
        .collect::<Vec<_>>();
    if has_root_component {
        let root_ty = &item.ident;
        component_descriptors.push(quote! {
            ::phenix_sdk::StaticComponentDescriptor::derived::<#root_ty>(
                &Self::plugin_id(),
                "root",
            )
        });
    }
    let root_imports = contributions.imports.iter().map(|import| {
        let field = &import.field;
        let ty = &import.ty;
        match import.authority.as_ref() {
            Some(authority) => quote! {
                ::phenix_sdk::StaticComponentImport::with_authority::<#ty>(
                    stringify!(#field),
                    #authority,
                )
            },
            None => quote! {
                ::phenix_sdk::StaticComponentImport::of::<#ty>(stringify!(#field))
            },
        }
    });
    let root_hosts = contributions.hosts.iter().map(|host| {
        let field = &host.field;
        let ty = &host.ty;
        match host.authority.as_ref() {
            Some(authority) => quote! {
                ::phenix_sdk::StaticComponentHost::with_authority::<#ty>(
                    stringify!(#field),
                    #authority,
                )
            },
            None => quote! {
                ::phenix_sdk::StaticComponentHost::of::<#ty>(stringify!(#field))
            },
        }
    });
    let root_events = contributions.events.iter().map(|event| {
        let field = &event.field;
        let ty = &event.ty;
        let id = &event.event;
        quote! {
            ::phenix_sdk::StaticComponentEvent::of::<#ty>(#id, stringify!(#field))
        }
    });
    let resource_descriptors = contributions.resources.iter().map(|resource| {
        let field = &resource.field;
        let ty = &resource.ty;
        let features = &resource.features;
        if let Some(id) = &resource.id {
            quote! {
                ::phenix_sdk::StaticResourceDescriptor::explicit::<#ty>(
                    #id,
                    stringify!(#field),
                    [#(::phenix_sdk::BackendFeature::#features),*],
                )
            }
        } else {
            quote! {
                ::phenix_sdk::StaticResourceDescriptor::derived::<#ty>(
                    &Self::plugin_id(),
                    stringify!(#field),
                    [#(::phenix_sdk::BackendFeature::#features),*],
                )
            }
        }
    });
    let configuration = contributions.configuration.as_ref().map(|configuration| {
        let field = &configuration.field;
        let ty = &configuration.ty;
        quote! {
            Some(::phenix_sdk::StaticPluginConfigDescriptor::of::<#ty>(stringify!(#field)))
        }
    });
    let configuration = configuration.unwrap_or_else(|| quote!(None));
    let id = resolve_plugin_id(args, &item.ident)?;
    let name = &item.ident;
    let identity_impl = plugin_identity_impl(name, &id);
    let root_component_impl = has_root_component.then(|| {
        quote! {
            impl ::phenix_sdk::StaticComponentDefinition for #name {}

            impl ::phenix_sdk::StaticComponentImports for #name {
                fn imports() -> Vec<::phenix_sdk::StaticComponentImport> {
                    vec![#(#root_imports),*]
                }

                fn hosts() -> Vec<::phenix_sdk::StaticComponentHost> {
                    vec![#(#root_hosts),*]
                }

                fn events() -> Vec<::phenix_sdk::StaticComponentEvent> {
                    vec![#(#root_events),*]
                }
            }

            impl ::phenix_sdk::StaticComponentBehavior for #name {}
        }
    });

    Ok(quote! {
        #item

        #identity_impl

        #(#dependency_aliases)*

        #dependency_access

        #root_component_impl

        impl ::phenix_sdk::StaticPluginDefinition for #name {
            fn descriptor() -> ::phenix_sdk::StaticPluginDescriptor {
                ::phenix_sdk::StaticPluginDescriptor {
                    id: Self::plugin_id(),
                    definition: concat!(module_path!(), "::", stringify!(#name)),
                    version: #version,
                    execution: #execution,
                    maximum_authority: #authority,
                    dependencies: vec![
                        #(::phenix_sdk::StaticPluginDependency::of::<#dependency_types>()),*
                    ],
                    embedded_factory: None,
                }
            }
        }

        impl ::phenix_sdk::StaticPluginConfiguration for #name {
            fn configuration() -> Option<::phenix_sdk::StaticPluginConfigDescriptor> {
                #configuration
            }
        }

        impl ::phenix_sdk::StaticPluginComponents for #name {
            fn components() -> Vec<::phenix_sdk::StaticComponentDescriptor> {
                vec![#(#component_descriptors),*]
            }
        }

        impl ::phenix_sdk::StaticPluginResources for #name {
            fn resources() -> Vec<::phenix_sdk::StaticResourceDescriptor> {
                vec![#(#resource_descriptors),*]
            }
        }
    })
}

fn expand_module(args: TokenStream, mut item: ItemMod) -> syn::Result<TokenStream> {
    let execution = plugin_execution(args.clone())?;
    let authority = plugin_authority(args.clone())?;
    let version = plugin_version(args.clone())?;
    let resource_only = plugin_execution_is_resource_only(args.clone())?;
    let runtime_hosted = plugin_execution_is_runtime_hosted(args.clone())?;
    let id = resolve_plugin_id(args, &item.ident)?;
    if resource_only {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "resource-only plugins cannot use the stateless embedded-handler form",
        ));
    }
    if runtime_hosted {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "runtime-hosted plugins cannot use the stateless embedded-handler form",
        ));
    }
    let Some((_, items)) = item.content.as_mut() else {
        return Err(syn::Error::new_spanned(
            &item,
            "stateless Phenix plugins must use an inline module so authoring contributions stay visible to the macro",
        ));
    };
    if items.iter().any(defines_generated_plugin_type) {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "stateless plugin modules reserve the names Plugin and Component for generated zero-sized types",
        ));
    }

    let contributions = module_contributions(items)?;
    let export_descriptors = contributions
        .exports
        .iter()
        .map(|export| {
            export_descriptor(
                &export.function,
                &export.export,
                &export.decoded_request,
                &export.response,
                export.domain_error.as_ref(),
            )
        })
        .collect::<Vec<_>>();
    let dispatch_arms = contributions.exports.iter().map(|export| {
        let interface = export.export.interface_expression();
        let function = &export.function;
        let decoded_request = &export.decoded_request;
        let request = match export.projection {
            StatelessRequestProjection::Projected => quote!(request),
            StatelessRequestProjection::ExplicitProject => quote!(::phenix_sdk::Project(request)),
            StatelessRequestProjection::Exact => quote!(::phenix_sdk::Exact(request)),
        };
        let call = match (export.has_context, export.has_request) {
            (true, true) => quote!(#function(&call_context, #request)),
            (true, false) => quote!(#function(&call_context)),
            (false, true) => quote!(#function(#request)),
            (false, false) => quote!(#function()),
        };
        let request_binding = if export.has_request {
            quote!(request)
        } else {
            quote!(_request)
        };
        let handler = if export.domain_error.is_some() {
            quote!(|#request_binding: #decoded_request| #call)
        } else {
            quote!(|#request_binding: #decoded_request| Ok::<_, String>(#call))
        };
        let dispatch = match export.projection {
            StatelessRequestProjection::Exact => quote!(::phenix_sdk::dispatch_exact_provider),
            StatelessRequestProjection::Projected | StatelessRequestProjection::ExplicitProject => {
                quote!(::phenix_sdk::dispatch_projected_provider)
            }
        };

        quote! {
            {
                let interface = #interface;
                if service.as_str() == interface.as_str() {
                    let call_context = ::phenix_sdk::CallContext {
                        authority: host.authority(),
                        graph_generation: host.graph_generation(),
                    };
                    return #dispatch(host, &interface, input, #handler);
                }
            }
        }
    });
    let value_descriptors = contributions
        .values
        .iter()
        .map(|(function, value, value_type)| {
            let id = &value.id;
            let public = value.public;
            quote! {
                ::phenix_sdk::StaticComponentValue::of::<#value_type>(
                    #id, stringify!(#function), #public,
                )
            }
        });

    let identity: Item = parse_quote! {
        #[doc(hidden)]
        pub struct Plugin;
    };
    let component: Item = parse_quote! {
        #[doc(hidden)]
        pub struct Component;
    };
    let identity_impl: Item = parse_quote! {
        impl Plugin {
            #[must_use]
            pub fn plugin_id() -> ::phenix_sdk::PluginId {
                ::phenix_sdk::PluginId::parse(#id)
                    .expect("plugin attribute validated the static plugin id")
            }

            #[must_use]
            pub fn component_id() -> ::phenix_sdk::ComponentId {
                ::phenix_sdk::ComponentId::parse(#id)
                    .expect("plugin attribute validated the default component id")
            }
        }
    };
    let definition_impl: Item = parse_quote! {
        impl ::phenix_sdk::StaticPluginDefinition for Plugin {
            fn descriptor() -> ::phenix_sdk::StaticPluginDescriptor {
                ::phenix_sdk::StaticPluginDescriptor {
                    id: Self::plugin_id(),
                    definition: concat!(module_path!(), "::Plugin"),
                    version: #version,
                    execution: #execution,
                    maximum_authority: #authority,
                    dependencies: Vec::new(),
                    embedded_factory: Some(
                        <Plugin as ::phenix_sdk::StaticPluginFactory>::factory,
                    ),
                }
            }
        }
    };
    let configuration_impl: Item = parse_quote! {
        impl ::phenix_sdk::StaticPluginConfiguration for Plugin {
            fn configuration() -> Option<::phenix_sdk::StaticPluginConfigDescriptor> {
                None
            }
        }
    };
    let resources_impl: Item = parse_quote! {
        impl ::phenix_sdk::StaticPluginResources for Plugin {
            fn resources() -> Vec<::phenix_sdk::StaticResourceDescriptor> { Vec::new() }
        }
    };
    let component_definition: Item = parse_quote! {
        impl ::phenix_sdk::StaticComponentDefinition for Component {}
    };
    let component_imports: Item = parse_quote! {
        impl ::phenix_sdk::StaticComponentImports for Component {}
    };
    let component_behavior: Item = syn::parse2(quote! {
        impl ::phenix_sdk::StaticComponentBehavior for Component {
            fn exports() -> Vec<::phenix_sdk::StaticComponentExport> {
                vec![#(#export_descriptors),*]
            }

            fn values() -> Vec<::phenix_sdk::StaticComponentValue> {
                vec![#(#value_descriptors),*]
            }
        }
    })?;
    let instance_impl: Item = parse_quote! {
        impl ::phenix_sdk::__phenix_plugin::PluginInstance for Plugin {
            fn start(
                &mut self,
                _host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<(), String> {
                Ok(())
            }

            fn invoke(
                &mut self,
                service: &::phenix_sdk::__phenix_plugin::ServiceId,
                input: &[u8],
                host: &::phenix_sdk::__phenix_plugin::PluginHost<'_>,
            ) -> Result<Vec<u8>, String> {
                #(#dispatch_arms)*
                Err(format!("unsupported stateless plugin service: {service}"))
            }
        }
    };
    let factory_impl: Item = parse_quote! {
        impl ::phenix_sdk::StaticPluginFactory for Plugin {
            fn factory() -> Box<dyn ::phenix_sdk::__phenix_plugin::PluginInstance> {
                Box::new(Self)
            }
        }
    };
    let components_impl: Item = parse_quote! {
        impl ::phenix_sdk::StaticPluginComponents for Plugin {
            fn components() -> Vec<::phenix_sdk::StaticComponentDescriptor> {
                vec![::phenix_sdk::StaticComponentDescriptor::explicit::<Component>(
                    #id,
                    "default",
                )]
            }
        }
    };
    items.extend([
        identity,
        component,
        identity_impl,
        definition_impl,
        configuration_impl,
        resources_impl,
        component_definition,
        component_imports,
        component_behavior,
        instance_impl,
        factory_impl,
        components_impl,
    ]);

    Ok(quote!(#item))
}

#[derive(Default)]
struct ModuleContributions {
    exports: Vec<StatelessExportContribution>,
    values: Vec<(Ident, StatelessValueContribution, Type)>,
}

struct StatelessExportContribution {
    function: Ident,
    export: ExportContribution,
    response: Type,
    decoded_request: Type,
    projection: StatelessRequestProjection,
    has_context: bool,
    has_request: bool,
    domain_error: Option<Type>,
}

#[derive(Clone, Copy)]
enum StatelessRequestProjection {
    Projected,
    ExplicitProject,
    Exact,
}

struct StatelessValueContribution {
    id: LitStr,
    public: bool,
}

fn module_contributions(items: &mut [Item]) -> syn::Result<ModuleContributions> {
    let mut contributions = ModuleContributions::default();
    for item in items {
        let Item::Fn(function) = item else {
            continue;
        };
        let mut retained = Vec::new();
        let mut contribution = None;
        for attribute in std::mem::take(&mut function.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            if contribution.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "a stateless plugin function may declare only one Phenix contribution",
                ));
            }
            contribution = Some(parse_stateless_contribution(&attribute)?);
        }
        function.attrs = retained;
        match contribution {
            Some(StatelessContribution::Export(export)) => {
                let (request, response) = export_signature_types(&function.sig, false)?;
                let (decoded_request, projection) = stateless_request_projection(&request)?;
                let has_context = function
                    .sig
                    .inputs
                    .first()
                    .is_some_and(is_call_context_parameter);
                let has_request = function.sig.inputs.len() > usize::from(has_context);
                contributions.exports.push(StatelessExportContribution {
                    function: function.sig.ident.clone(),
                    export: *export,
                    response,
                    decoded_request,
                    projection,
                    has_context,
                    has_request,
                    domain_error: export_error_type(&function.sig.output)?,
                });
            }
            Some(StatelessContribution::Value(value)) => {
                validate_stateless_value_signature(&function.sig, &value)?;
                let value_type = value_response_type(&function.sig)?;
                contributions
                    .values
                    .push((function.sig.ident.clone(), value, value_type));
            }
            None => {}
        }
    }
    Ok(contributions)
}

fn stateless_request_projection(request: &Type) -> syn::Result<(Type, StatelessRequestProjection)> {
    let Type::Path(path) = request else {
        return Ok((request.clone(), StatelessRequestProjection::Projected));
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok((request.clone(), StatelessRequestProjection::Projected));
    };
    let projection = if segment.ident == "Exact" {
        StatelessRequestProjection::Exact
    } else if segment.ident == "Project" {
        StatelessRequestProjection::ExplicitProject
    } else {
        return Ok((request.clone(), StatelessRequestProjection::Projected));
    };
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "Project and Exact stateless request wrappers require exactly one payload type",
        ));
    };
    if arguments.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            arguments,
            "Project and Exact stateless request wrappers require exactly one payload type",
        ));
    }
    let Some(syn::GenericArgument::Type(inner)) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            arguments,
            "Project and Exact stateless request wrappers require exactly one payload type",
        ));
    };
    Ok((inner.clone(), projection))
}

fn validate_stateless_value_signature(
    signature: &syn::Signature,
    value: &StatelessValueContribution,
) -> syn::Result<()> {
    if !value.public {
        return Ok(());
    }
    if !signature.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.generics,
            "public stateless values cannot be generic",
        ));
    }

    let mut inputs = signature.inputs.iter();
    if let Some(context) = inputs.next() {
        if !is_stateless_read_context_parameter(context) {
            return Err(syn::Error::new_spanned(
                context,
                "public stateless values accept only an optional &ReadContext",
            ));
        }
    }
    if inputs.next().is_some() {
        return Err(syn::Error::new_spanned(
            signature,
            "public stateless values accept only an optional &ReadContext",
        ));
    }
    Ok(())
}

fn is_stateless_read_context_parameter(argument: &FnArg) -> bool {
    let FnArg::Typed(argument) = argument else {
        return false;
    };
    let Type::Reference(reference) = argument.ty.as_ref() else {
        return false;
    };
    reference.mutability.is_none()
        && matches!(reference.elem.as_ref(), Type::Path(context)
            if context.qself.is_none()
                && context.path.segments.last().is_some_and(|segment| segment.ident == "ReadContext"))
}

enum StatelessContribution {
    Export(Box<ExportContribution>),
    Value(StatelessValueContribution),
}

fn parse_stateless_contribution(attribute: &Attribute) -> syn::Result<StatelessContribution> {
    let Meta::List(meta) = &attribute.meta else {
        return Err(syn::Error::new_spanned(
            attribute,
            "stateless plugin function attributes must use #[phenix(...)] syntax",
        ));
    };
    let arguments = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let Some(Meta::List(kind)) = arguments.first() else {
        return Err(syn::Error::new_spanned(
            attribute,
            "stateless plugin function contribution must begin with export(...) or value(...)",
        ));
    };
    if kind.path.is_ident("export") {
        return parse_export(attribute)
            .map(Box::new)
            .map(StatelessContribution::Export);
    }
    if !kind.path.is_ident("value") {
        return Err(syn::Error::new_spanned(
            &kind.path,
            "stateless plugins support export(...) and value(...) functions",
        ));
    }

    let id = syn::parse2::<LitStr>(kind.tokens.clone())?;
    validate_interface_id(&id.value()).map_err(|error| syn::Error::new_spanned(&id, error))?;
    let mut public = false;
    for modifier in arguments.iter().skip(1) {
        let Meta::Path(path) = modifier else {
            return Err(syn::Error::new_spanned(
                modifier,
                "value modifiers must be bare flags",
            ));
        };
        if !path.is_ident("public") || public {
            return Err(syn::Error::new_spanned(
                path,
                "value only supports one public modifier",
            ));
        }
        public = true;
    }
    Ok(StatelessContribution::Value(StatelessValueContribution {
        id,
        public,
    }))
}

fn plugin_identity_impl(name: &Ident, id: &LitStr) -> TokenStream {
    quote! {
        impl #name {
            #[must_use]
            pub fn plugin_id() -> ::phenix_sdk::PluginId {
                ::phenix_sdk::PluginId::parse(#id)
                    .expect("plugin attribute validated the static plugin id")
            }

            #[must_use]
            pub fn component_id() -> ::phenix_sdk::ComponentId {
                ::phenix_sdk::ComponentId::parse(#id)
                    .expect("plugin attribute validated the default component id")
            }
        }
    }
}

fn defines_generated_plugin_type(item: &Item) -> bool {
    let ident = match item {
        Item::Struct(item) => Some(&item.ident),
        Item::Enum(item) => Some(&item.ident),
        Item::Union(item) => Some(&item.ident),
        Item::Type(item) => Some(&item.ident),
        _ => None,
    };
    ident.is_some_and(|ident| ident == "Plugin" || ident == "Component")
}

fn resolve_plugin_id(args: TokenStream, item: &Ident) -> syn::Result<LitStr> {
    if let Some(id) = explicit_plugin_id(args)? {
        return Ok(id);
    }

    let package = std::env::var("CARGO_PKG_NAME")
        .map_err(|_| syn::Error::new_spanned(item, "plugin package identity is unavailable"))?;
    let id = default_plugin_id(&package).ok_or_else(|| {
        syn::Error::new_spanned(
            item,
            "plugin without an explicit id requires a phenix-plugin-* package name",
        )
    })?;
    Ok(LitStr::new(&id, item.span()))
}

fn plugin_execution(args: TokenStream) -> syn::Result<TokenStream> {
    if args.is_empty() || syn::parse2::<LitStr>(args.clone()).is_ok() {
        return Ok(quote!(::phenix_sdk::PluginExecution::Embedded));
    }

    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut execution = None;
    for argument in args {
        let Meta::NameValue(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "plugin attributes must use a string id or key = value syntax",
            ));
        };
        if argument.path.is_ident("id")
            || argument.path.is_ident("authority")
            || argument.path.is_ident("version")
        {
            continue;
        }
        if !argument.path.is_ident("execution") {
            return Err(syn::Error::new_spanned(
                argument.path,
                "unsupported plugin attribute",
            ));
        }
        if execution.is_some() {
            return Err(syn::Error::new_spanned(
                argument,
                "duplicate plugin execution mode",
            ));
        }
        execution = Some(argument.value);
    }

    Ok(execution
        .map(|execution| quote!(#execution))
        .unwrap_or_else(|| quote!(::phenix_sdk::PluginExecution::Embedded)))
}

fn plugin_execution_is_resource_only(args: TokenStream) -> syn::Result<bool> {
    if args.is_empty() || syn::parse2::<LitStr>(args.clone()).is_ok() {
        return Ok(false);
    }

    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    Ok(args.into_iter().any(|argument| {
        let Meta::NameValue(argument) = argument else {
            return false;
        };
        if !argument.path.is_ident("execution") {
            return false;
        }
        let Expr::Path(execution) = argument.value else {
            return false;
        };
        execution
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "ResourceOnly")
    }))
}

fn plugin_execution_is_runtime_hosted(args: TokenStream) -> syn::Result<bool> {
    if args.is_empty() || syn::parse2::<LitStr>(args.clone()).is_ok() {
        return Ok(false);
    }

    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    Ok(args.into_iter().any(|argument| {
        let Meta::NameValue(argument) = argument else {
            return false;
        };
        if !argument.path.is_ident("execution") {
            return false;
        }
        let Expr::Struct(execution) = argument.value else {
            return false;
        };
        execution
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Runtime")
    }))
}

fn plugin_authority(args: TokenStream) -> syn::Result<TokenStream> {
    if args.is_empty() || syn::parse2::<LitStr>(args.clone()).is_ok() {
        return Ok(quote!(::phenix_sdk::Authority::default()));
    }

    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut authority = None;
    for argument in args {
        let Meta::NameValue(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "plugin attributes must use a string id or key = value syntax",
            ));
        };
        if argument.path.is_ident("id")
            || argument.path.is_ident("execution")
            || argument.path.is_ident("version")
        {
            continue;
        }
        if !argument.path.is_ident("authority") {
            return Err(syn::Error::new_spanned(
                argument.path,
                "unsupported plugin attribute",
            ));
        }
        if authority.is_some() {
            return Err(syn::Error::new_spanned(
                argument,
                "duplicate plugin authority",
            ));
        }
        authority = Some(argument.value);
    }

    Ok(authority
        .map(|authority| quote!(#authority))
        .unwrap_or_else(|| quote!(::phenix_sdk::Authority::default())))
}

fn plugin_version(args: TokenStream) -> syn::Result<u32> {
    if args.is_empty() || syn::parse2::<LitStr>(args.clone()).is_ok() {
        return Ok(1);
    }

    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut version = None;
    for argument in args {
        let Meta::NameValue(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "plugin attributes must use a string id or key = value syntax",
            ));
        };
        if argument.path.is_ident("id")
            || argument.path.is_ident("execution")
            || argument.path.is_ident("authority")
        {
            continue;
        }
        if !argument.path.is_ident("version") {
            return Err(syn::Error::new_spanned(
                argument.path,
                "unsupported plugin attribute",
            ));
        }
        if version.is_some() {
            return Err(syn::Error::new_spanned(
                argument,
                "duplicate plugin version",
            ));
        }
        let Expr::Lit(ExprLit {
            lit: Lit::Int(value),
            ..
        }) = &argument.value
        else {
            return Err(syn::Error::new_spanned(
                &argument.value,
                "plugin version must be a positive integer literal",
            ));
        };
        let parsed = value
            .base10_parse::<u32>()
            .map_err(|_| syn::Error::new_spanned(value, "plugin version must fit in u32"))?;
        if parsed == 0 {
            return Err(syn::Error::new_spanned(
                value,
                "plugin version must be a positive integer",
            ));
        }
        version = Some(parsed);
    }

    Ok(version.unwrap_or(1))
}

fn explicit_plugin_id(args: TokenStream) -> syn::Result<Option<LitStr>> {
    if args.is_empty() {
        return Ok(None);
    }

    if let Ok(value) = syn::parse2::<LitStr>(args.clone()) {
        validate_static_id(&value.value(), "plugin")
            .map_err(|error| syn::Error::new_spanned(&value, error))?;
        return Ok(Some(value));
    }

    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut id = None;
    for argument in args {
        let Meta::NameValue(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "plugin attributes must use a string id or key = value syntax",
            ));
        };
        if argument.path.is_ident("execution")
            || argument.path.is_ident("authority")
            || argument.path.is_ident("version")
        {
            continue;
        }
        if !argument.path.is_ident("id") {
            return Err(syn::Error::new_spanned(
                argument.path,
                "unsupported plugin attribute",
            ));
        }
        if id.is_some() {
            return Err(syn::Error::new_spanned(argument, "duplicate plugin id"));
        }
        let value = string_literal(argument.value, "plugin id must be a string literal")?;
        validate_static_id(&value.value(), "plugin")
            .map_err(|error| syn::Error::new_spanned(&value, error))?;
        id = Some(value);
    }
    Ok(id)
}

#[derive(Default)]
struct FieldContributions {
    dependencies: Vec<DependencyContribution>,
    configuration: Option<ConfigContribution>,
    components: Vec<ComponentContribution>,
    resources: Vec<ResourceContribution>,
    imports: Vec<ImportContribution>,
    hosts: Vec<ImportContribution>,
    events: Vec<EventFieldContribution>,
}

struct DependencyContribution {
    field: Ident,
    ty: Type,
}

struct ConfigContribution {
    field: Ident,
    ty: Type,
}

struct ComponentContribution {
    field: Ident,
    ty: Type,
    id: Option<LitStr>,
}

struct ResourceContribution {
    field: Ident,
    ty: Type,
    id: Option<LitStr>,
    features: Vec<Ident>,
}

struct ImportContribution {
    field: Ident,
    ty: Type,
    authority: Option<Expr>,
}

struct EventFieldContribution {
    field: Ident,
    ty: Type,
    event: LitStr,
}

enum FieldRole {
    Dependency,
    Config,
    Import {
        authority: Option<Expr>,
    },
    Host {
        authority: Option<Expr>,
    },
    Event {
        event: LitStr,
    },
    Component {
        id: Option<LitStr>,
    },
    Resource {
        id: Option<LitStr>,
        features: Vec<Ident>,
    },
}

fn field_contributions(item: &mut ItemStruct) -> syn::Result<FieldContributions> {
    let Fields::Named(fields) = &mut item.fields else {
        return Ok(FieldContributions::default());
    };
    let mut contributions = FieldContributions::default();

    for field in &mut fields.named {
        let mut retained = Vec::new();
        let mut role = None;
        for attribute in std::mem::take(&mut field.attrs) {
            if !attribute.path().is_ident("phenix") {
                retained.push(attribute);
                continue;
            }
            if role.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "a plugin field may have only one Phenix role",
                ));
            }
            role = Some(field_role(&attribute)?);
        }
        field.attrs = retained;

        match role {
            Some(FieldRole::Dependency) => {
                let name = field.ident.clone().expect("named field has an identifier");
                contributions.dependencies.push(DependencyContribution {
                    field: name,
                    ty: field.ty.clone(),
                });
            }
            Some(FieldRole::Config) => {
                if contributions.configuration.is_some() {
                    return Err(syn::Error::new_spanned(
                        field,
                        "a plugin may declare only one configuration field",
                    ));
                }
                let name = field.ident.clone().expect("named field has an identifier");
                contributions.configuration = Some(ConfigContribution {
                    field: name,
                    ty: field.ty.clone(),
                });
            }
            Some(FieldRole::Component { id }) => {
                let name = field.ident.clone().expect("named field has an identifier");
                contributions.components.push(ComponentContribution {
                    field: name,
                    ty: field.ty.clone(),
                    id,
                });
            }
            Some(FieldRole::Resource { id, features }) => {
                let name = field.ident.clone().expect("named field has an identifier");
                contributions.resources.push(ResourceContribution {
                    field: name,
                    ty: field.ty.clone(),
                    id,
                    features,
                });
            }
            Some(FieldRole::Import { authority }) => {
                let name = field.ident.clone().expect("named field has an identifier");
                contributions.imports.push(ImportContribution {
                    field: name,
                    ty: field.ty.clone(),
                    authority,
                });
            }
            Some(FieldRole::Host { authority }) => {
                let name = field.ident.clone().expect("named field has an identifier");
                contributions.hosts.push(ImportContribution {
                    field: name,
                    ty: field.ty.clone(),
                    authority,
                });
            }
            Some(FieldRole::Event { event }) => {
                let name = field.ident.clone().expect("named field has an identifier");
                contributions.events.push(EventFieldContribution {
                    field: name,
                    ty: field.ty.clone(),
                    event,
                });
            }
            None => {}
        }
    }

    Ok(contributions)
}

fn validate_nested_ids(plugin_id: &LitStr, contributions: &FieldContributions) -> syn::Result<()> {
    let derived = |field: &Ident| format!("{}.{}", plugin_id.value(), field);

    let mut component_ids = std::collections::BTreeSet::new();
    if !contributions.imports.is_empty()
        || !contributions.hosts.is_empty()
        || !contributions.events.is_empty()
    {
        component_ids.insert(format!("{}.root", plugin_id.value()));
    }
    for component in &contributions.components {
        let id = component
            .id
            .as_ref()
            .map(LitStr::value)
            .unwrap_or_else(|| derived(&component.field));
        if !component_ids.insert(id.clone()) {
            return Err(syn::Error::new_spanned(
                &component.field,
                format!("duplicate component id `{id}`"),
            ));
        }
    }

    let mut resource_ids = std::collections::BTreeSet::new();
    for resource in &contributions.resources {
        let id = resource
            .id
            .as_ref()
            .map(LitStr::value)
            .unwrap_or_else(|| derived(&resource.field));
        if !resource_ids.insert(id.clone()) {
            return Err(syn::Error::new_spanned(
                &resource.field,
                format!("duplicate resource id `{id}`"),
            ));
        }
    }

    Ok(())
}

fn field_role(attribute: &Attribute) -> syn::Result<FieldRole> {
    let Meta::List(meta) = &attribute.meta else {
        return Err(syn::Error::new_spanned(
            attribute,
            "plugin field attributes must use #[phenix(...)] syntax",
        ));
    };
    let arguments = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(meta.tokens.clone())?;
    let mut arguments = arguments.into_iter();
    let Some(first) = arguments.next() else {
        return Err(syn::Error::new_spanned(
            attribute,
            "plugin field attribute must begin with a field role",
        ));
    };
    if let Meta::List(event) = &first {
        if event.path.is_ident("event") {
            if let Some(argument) = arguments.next() {
                return Err(syn::Error::new_spanned(
                    argument,
                    "event fields do not accept additional metadata",
                ));
            }
            let event = syn::parse2::<LitStr>(event.tokens.clone())?;
            validate_static_id(&event.value(), "event")
                .map_err(|error| syn::Error::new_spanned(&event, error))?;
            return Ok(FieldRole::Event { event });
        }
    }
    let Meta::Path(role) = first else {
        return Err(syn::Error::new_spanned(
            attribute,
            "plugin field attribute must begin with a field role",
        ));
    };

    if role.is_ident("dep") || role.is_ident("config") {
        if let Some(argument) = arguments.next() {
            let role_name = if role.is_ident("dep") {
                "dependency"
            } else {
                "configuration"
            };
            return Err(syn::Error::new_spanned(
                argument,
                format!("{role_name} fields do not accept metadata"),
            ));
        }
        return Ok(if role.is_ident("dep") {
            FieldRole::Dependency
        } else {
            FieldRole::Config
        });
    }
    if role.is_ident("import") || role.is_ident("host") {
        let is_import = role.is_ident("import");
        let kind = if is_import { "import" } else { "host" };
        let mut authority = None;
        for argument in arguments {
            let Meta::NameValue(argument) = argument else {
                return Err(syn::Error::new_spanned(
                    argument,
                    format!("{kind} metadata must use authority = <expression>"),
                ));
            };
            if !argument.path.is_ident("authority") {
                return Err(syn::Error::new_spanned(
                    argument.path,
                    format!("unsupported {kind} metadata"),
                ));
            }
            if authority.is_some() {
                return Err(syn::Error::new_spanned(
                    argument,
                    format!("duplicate {kind} authority"),
                ));
            }
            authority = Some(argument.value);
        }
        return Ok(if is_import {
            FieldRole::Import { authority }
        } else {
            FieldRole::Host { authority }
        });
    }
    let kind = if role.is_ident("component") {
        "component"
    } else if role.is_ident("resource") {
        "resource"
    } else {
        return Err(syn::Error::new_spanned(
            role,
            "unsupported plugin field attribute",
        ));
    };

    let mut id = None;
    let mut features = Vec::new();
    for argument in arguments {
        if kind == "resource" {
            if let Meta::List(feature_list) = &argument {
                if feature_list.path.is_ident("features") {
                    if !features.is_empty() {
                        return Err(syn::Error::new_spanned(
                            argument,
                            "duplicate resource features",
                        ));
                    }
                    let values = Punctuated::<Ident, Token![,]>::parse_terminated
                        .parse2(feature_list.tokens.clone())?;
                    for feature in values {
                        if !matches!(
                            feature.to_string().as_str(),
                            "Transactions"
                                | "UniqueKeys"
                                | "ForeignKeys"
                                | "OrderedAppend"
                                | "IndexedRange"
                                | "Migrations"
                        ) {
                            return Err(syn::Error::new_spanned(
                                feature,
                                "unsupported resource backend feature",
                            ));
                        }
                        features.push(feature);
                    }
                    continue;
                }
            }
        }

        let Meta::NameValue(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                format!("{kind} metadata must use key = value syntax"),
            ));
        };
        if !argument.path.is_ident("id") {
            return Err(syn::Error::new_spanned(
                argument.path,
                format!("unsupported {kind} metadata"),
            ));
        }
        if id.is_some() {
            return Err(syn::Error::new_spanned(
                argument,
                format!("duplicate {kind} id"),
            ));
        }
        let value = string_literal(argument.value, "field id must be a string literal")?;
        validate_static_id(&value.value(), kind)
            .map_err(|error| syn::Error::new_spanned(&value, error))?;
        id = Some(value);
    }
    Ok(if kind == "component" {
        FieldRole::Component { id }
    } else {
        FieldRole::Resource { id, features }
    })
}

fn string_literal(value: Expr, message: &'static str) -> syn::Result<LitStr> {
    let Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    }) = value
    else {
        return Err(syn::Error::new_spanned(value, message));
    };
    Ok(value)
}

fn validate_static_id(value: &str, kind: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{kind} id must not be empty"));
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
    }) {
        Ok(())
    } else {
        Err(format!("{kind} id contains unsupported characters"))
    }
}

fn default_plugin_id(package: &str) -> Option<String> {
    package
        .strip_prefix("phenix-plugin-")
        .filter(|name| !name.is_empty())
        .map(|name| format!("phenix.{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    use syn::parse_quote;

    #[test]
    fn lifecycle_rejects_generic_methods() {
        let method: syn::ImplItemFn = parse_quote! {
            fn start<T>(
                &mut self,
                _context: &phenix_sdk::PluginContext<'_, '_, ()>,
            ) -> Result<(), String> {
                Ok(())
            }
        };

        let error = validate_lifecycle_signature(&method).unwrap_err();
        assert_eq!(
            error.to_string(),
            "plugin lifecycle methods cannot be generic"
        );
    }

    #[test]
    fn package_name_derives_stable_plugin_id() {
        assert_eq!(
            default_plugin_id("phenix-plugin-session-tree").as_deref(),
            Some("phenix.session-tree")
        );
        assert_eq!(default_plugin_id("phenix-sdk"), None);
    }

    #[test]
    fn canonical_positional_plugin_id_is_accepted() {
        let id = explicit_plugin_id(quote!("phenix.example"))
            .unwrap()
            .unwrap();
        assert_eq!(id.value(), "phenix.example");
    }

    #[test]
    fn keyed_plugin_id_remains_supported() {
        let id = explicit_plugin_id(quote!(id = "phenix.example"))
            .unwrap()
            .unwrap();
        assert_eq!(id.value(), "phenix.example");
    }

    #[test]
    fn plugin_version_is_preserved_in_stateful_and_stateless_descriptors() {
        let stateful = expand(
            quote!(id = "phenix.versioned", version = 7),
            quote! { struct Plugin; },
        )
        .unwrap()
        .to_string();
        let stateless = expand(
            quote!(id = "phenix.versioned-stateless", version = 9),
            quote! { mod plugin {} },
        )
        .unwrap()
        .to_string();

        assert!(stateful.contains("version : 7"));
        assert!(stateless.contains("version : 9"));
        assert!(stateful.contains("embedded_factory : None"));
        assert!(stateless.contains("embedded_factory : Some"));
    }

    #[test]
    fn plugin_version_rejects_zero_and_duplicates() {
        assert!(plugin_version(quote!(version = 0))
            .unwrap_err()
            .to_string()
            .contains("positive integer"));
        assert!(plugin_version(quote!(version = 1, version = 2))
            .unwrap_err()
            .to_string()
            .contains("duplicate plugin version"));
    }

    #[test]
    fn stateless_module_lowers_default_component_and_exports() {
        let output = expand(
            quote!("phenix.stateless"),
            quote! {
                pub mod plugin {
                    #[phenix(export("phenix.stateless.run@1"), public)]
                    pub fn run() {}
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("pub struct Plugin"));
        assert!(output.contains("pub struct Component"));
        assert!(output.contains("StaticComponentBehavior for Component"));
        assert!(output.contains("phenix.stateless.run@1"));
        assert!(!output.contains("phenix (export"));
    }

    #[test]
    fn stateless_module_rejects_malformed_structural_wrapper() {
        let error = expand(
            quote!("phenix.stateless"),
            quote! {
                pub mod plugin {
                    #[phenix(export("phenix.stateless.run@1"))]
                    pub fn run(request: Exact<String, Unexpected>) -> String {
                        String::new()
                    }
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("require exactly one payload type"));
    }

    #[test]
    fn stateless_module_lowers_public_values() {
        let output = expand(
            quote!("phenix.stateless"),
            quote! {
                pub mod plugin {
                    #[phenix(value("phenix.stateless.capabilities@1"), public)]
                    pub fn capabilities() {}
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticComponentBehavior for Component"));
        assert!(output.contains("fn values"));
        assert!(output.contains("StaticComponentValue :: of"));
        assert!(output.contains("phenix.stateless.capabilities@1"));
        assert!(output.contains("true"));
        assert!(!output.contains("phenix (value"));
    }

    #[test]
    fn stateless_public_value_accepts_read_context() {
        expand(
            quote!("phenix.stateless"),
            quote! {
                pub mod plugin {
                    #[phenix(value("phenix.stateless.capabilities@1"), public)]
                    pub fn capabilities(_context: &ReadContext) {}
                }
            },
        )
        .expect("public stateless values may use read-only context");
    }

    #[test]
    fn stateless_public_value_rejects_request_parameters() {
        let error = expand(
            quote!("phenix.stateless"),
            quote! {
                pub mod plugin {
                    #[phenix(value("phenix.stateless.capabilities@1"), public)]
                    pub fn capabilities(_request: String) {}
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("accept only an optional &ReadContext"));
    }

    #[test]
    fn stateless_public_value_rejects_generic_functions() {
        let error = expand(
            quote!("phenix.stateless"),
            quote! {
                pub mod plugin {
                    #[phenix(value("phenix.stateless.capabilities@1"), public)]
                    pub fn capabilities<T>() -> T {
                        todo!()
                    }
                }
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "public stateless values cannot be generic"
        );
    }

    #[test]
    fn stateless_module_reserves_generated_type_names() {
        let error = expand(
            quote!("phenix.stateless"),
            quote! {
                mod plugin {
                    struct Component;
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("reserve"));
    }

    #[test]
    fn direct_dependencies_are_exposed_only_under_their_field_namespace() {
        let output = expand(
            quote!("phenix.parent"),
            quote! {
                struct Plugin {
                    #[phenix(dep)]
                    sessions: phenix_plugin_sessions::Plugin,
                    #[phenix(dep)]
                    models: phenix_plugin_models::Plugin,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("pub mod plugin"));
        assert!(output.contains("pub mod dependencies"));
        assert!(output.contains("pub mod sessions"));
        assert!(output.contains(
            "type __PhenixDependency_Plugin_sessions = phenix_plugin_sessions :: Plugin"
        ));
        assert!(output.contains(
            "pub type Plugin = super :: super :: super :: __PhenixDependency_Plugin_sessions"
        ));
        assert!(output.contains("pub mod models"));
        assert!(output
            .contains("type __PhenixDependency_Plugin_models = phenix_plugin_models :: Plugin"));
        assert!(output.contains(
            "pub type Plugin = super :: super :: super :: __PhenixDependency_Plugin_models"
        ));
    }

    #[test]
    fn config_field_is_lowered_to_typed_schema_metadata() {
        let output = expand(
            quote!("phenix.configured"),
            quote! {
                struct Plugin {
                    #[phenix(config)]
                    config: Settings,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticPluginConfiguration for Plugin"));
        assert!(output.contains("StaticPluginConfigDescriptor :: of :: < Settings >"));
        assert!(output.contains("stringify ! (config)"));
        assert!(!output.contains("phenix (config"));
    }

    #[test]
    fn multiple_config_fields_are_rejected() {
        let mut item: ItemStruct = parse_quote! {
            struct Plugin {
                #[phenix(config)]
                first: Settings,
                #[phenix(config)]
                second: Settings,
            }
        };

        let error = match field_contributions(&mut item) {
            Ok(_) => panic!("multiple configuration fields must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("only one configuration field"));
    }

    #[test]
    fn component_field_is_lowered_without_leaking_helper_attribute() {
        let mut item: ItemStruct = parse_quote! {
            struct Plugin {
                #[phenix(component)]
                api: Api,
            }
        };

        let contributions = field_contributions(&mut item).unwrap();
        assert_eq!(contributions.components.len(), 1);
        assert_eq!(contributions.components[0].field, "api");
        assert!(contributions.components[0].id.is_none());
        assert!(item.fields.iter().all(|field| field.attrs.is_empty()));
    }

    #[test]
    fn component_field_preserves_explicit_stable_id() {
        let mut item: ItemStruct = parse_quote! {
            struct Plugin {
                #[phenix(component, id = "legacy.component")]
                api: Api,
            }
        };

        let contributions = field_contributions(&mut item).unwrap();
        assert_eq!(
            contributions.components[0].id.as_ref().unwrap().value(),
            "legacy.component"
        );
    }

    #[test]
    fn explicit_component_id_cannot_collide_with_derived_sibling_id() {
        let error = expand(
            quote!("phenix.components"),
            quote! {
                struct Plugin {
                    #[phenix(component)]
                    api: Api,
                    #[phenix(component, id = "phenix.components.api")]
                    compatibility: CompatibilityApi,
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("duplicate component id `phenix.components.api`"));
    }

    #[test]
    fn explicit_resource_id_cannot_collide_with_derived_sibling_id() {
        let error = expand(
            quote!("phenix.resources"),
            quote! {
                struct Plugin {
                    #[phenix(resource)]
                    state: phenix_sdk::Durable<State>,
                    #[phenix(resource, id = "phenix.resources.state")]
                    compatibility: phenix_sdk::Durable<CompatibilityState>,
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("duplicate resource id `phenix.resources.state`"));
    }

    #[test]
    fn resource_field_is_lowered_without_leaking_helper_attribute() {
        let mut item: ItemStruct = parse_quote! {
            struct Plugin {
                #[phenix(resource)]
                state: phenix_sdk::Durable<State>,
            }
        };

        let contributions = field_contributions(&mut item).unwrap();
        assert_eq!(contributions.resources.len(), 1);
        assert_eq!(contributions.resources[0].field, "state");
        assert!(contributions.resources[0].id.is_none());
        assert!(contributions.resources[0].features.is_empty());
        assert!(item.fields.iter().all(|field| field.attrs.is_empty()));
    }

    #[test]
    fn resource_field_preserves_required_backend_features() {
        let mut item: ItemStruct = parse_quote! {
            struct Plugin {
                #[phenix(resource, features(Transactions, Migrations))]
                state: phenix_sdk::Durable<State>,
            }
        };

        let contributions = field_contributions(&mut item).unwrap();
        let features = contributions.resources[0]
            .features
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(features, ["Transactions", "Migrations"]);
    }

    #[test]
    fn resource_field_rejects_unknown_backend_feature() {
        let mut item: ItemStruct = parse_quote! {
            struct Plugin {
                #[phenix(resource, features(Transactions, Telepathy))]
                state: phenix_sdk::Durable<State>,
            }
        };

        let error = match field_contributions(&mut item) {
            Ok(_) => panic!("unknown backend feature must be rejected"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("unsupported resource backend feature"));
    }

    #[test]
    fn plugin_expansion_emits_resource_descriptor_from_resource_field() {
        let output = expand(
            quote!("phenix.resource-owner"),
            quote! {
                struct Plugin {
                    #[phenix(resource, id = "legacy.state")]
                    state: phenix_sdk::Durable<State>,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("StaticPluginResources for Plugin"));
        assert!(output.contains("StaticResourceDescriptor :: explicit"));
        assert!(output.contains("legacy.state"));
        assert!(!output.contains("phenix (resource"));
    }

    #[test]
    fn resource_only_plugin_rejects_embedded_component_fields() {
        let error = expand(
            quote!(
                id = "phenix.resource-only",
                execution = ::phenix_sdk::PluginExecution::ResourceOnly
            ),
            quote! {
                struct Plugin {
                    #[phenix(component)]
                    api: Api,
                    #[phenix(resource)]
                    state: phenix_sdk::Durable<State>,
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("resource-only plugins cannot declare embedded component fields"));
    }

    #[test]
    fn resource_only_plugin_rejects_root_component_imports() {
        let error = expand(
            quote!(
                id = "phenix.resource-only",
                execution = ::phenix_sdk::PluginExecution::ResourceOnly
            ),
            quote! {
                struct Plugin {
                    #[phenix(import)]
                    models: Required<Call<Models, Request, Response>>,
                    #[phenix(resource)]
                    state: phenix_sdk::Durable<State>,
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("resource-only plugins cannot declare embedded component fields"));
    }

    #[test]
    fn resource_only_plugin_accepts_resource_fields_without_components() {
        let output = expand(
            quote!(
                id = "phenix.resource-only",
                execution = ::phenix_sdk::PluginExecution::ResourceOnly
            ),
            quote! {
                struct Plugin {
                    #[phenix(resource)]
                    state: phenix_sdk::Durable<State>,
                }
            },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("PluginExecution :: ResourceOnly"));
    }

    #[test]
    fn resource_only_plugin_rejects_stateless_embedded_handler_form() {
        let error = expand(
            quote!(
                id = "phenix.resource-only",
                execution = ::phenix_sdk::PluginExecution::ResourceOnly
            ),
            quote! { mod plugin {} },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("stateless embedded-handler form"));
    }

    #[test]
    fn runtime_hosted_plugin_rejects_embedded_component_fields() {
        let error = expand(
            quote!(
                id = "phenix.runtime-hosted",
                execution = ::phenix_sdk::PluginExecution::Runtime {
                    runtime: runtime_id(),
                    artifact: artifact(),
                }
            ),
            quote! {
                struct Plugin {
                    #[phenix(component)]
                    api: Api,
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("runtime-hosted plugins cannot declare embedded component fields"));
    }

    #[test]
    fn runtime_hosted_plugin_rejects_root_component_events() {
        let error = expand(
            quote!(
                id = "phenix.runtime-hosted",
                execution = ::phenix_sdk::PluginExecution::Runtime {
                    runtime: runtime_id(),
                    artifact: artifact(),
                }
            ),
            quote! {
                struct Plugin {
                    #[phenix(event("phenix.runtime-hosted.changed"))]
                    changed: Emit<Response>,
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("runtime-hosted plugins cannot declare embedded component fields"));
    }

    #[test]
    fn runtime_hosted_plugin_accepts_metadata_without_embedded_components() {
        let output = expand(
            quote!(
                id = "phenix.runtime-hosted",
                execution = ::phenix_sdk::PluginExecution::Runtime {
                    runtime: runtime_id(),
                    artifact: artifact(),
                }
            ),
            quote! { struct Plugin; },
        )
        .unwrap()
        .to_string();

        assert!(output.contains("PluginExecution :: Runtime"));
    }

    #[test]
    fn runtime_hosted_plugin_rejects_stateless_embedded_handler_form() {
        let error = expand(
            quote!(
                id = "phenix.runtime-hosted",
                execution = ::phenix_sdk::PluginExecution::Runtime {
                    runtime: runtime_id(),
                    artifact: artifact(),
                }
            ),
            quote! { mod plugin {} },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("runtime-hosted plugins cannot use the stateless embedded-handler form"));
    }
}
