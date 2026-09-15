#[path = "plugin_attr_legacy.rs"]
mod legacy;

use proc_macro2::TokenStream;

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    legacy::expand(args, input)
}
