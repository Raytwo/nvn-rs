use syn::{parse::ParseStream, parse_macro_input};
use syn::parse::Parse;
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, ToTokens};

// originally by jam1garner
fn remove_mut(arg: &syn::FnArg) -> syn::FnArg {
    let mut arg = arg.clone();
    if let syn::FnArg::Typed(ref mut arg) = arg {
        if let syn::Pat::Ident(ref mut arg) = *arg.pat {
            arg.by_ref = None;
            arg.mutability = None;
            arg.subpat = None;
        }
    }
    arg
}

struct NVNStructArgs {
    pub opaque_size: syn::Expr,
    pub resolver: syn::Path
}

impl Parse for NVNStructArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let opaque_size = input.parse()?;
        let _: syn::Token![,] = input.parse()?;
        let resolver = input.parse()?;
        Ok(Self {
            opaque_size,
            resolver
        })
    }
}

struct NVNProcInfo {
    pub args: Vec<syn::FnArg>,
    pub owner_path: syn::Ident,
    pub fn_ident: syn::Ident,
    pub return_tokens: TokenStream2,
    pub is_const: bool,
    pub resolver_path: syn::Path
}

impl NVNProcInfo {
    pub fn generate_resolver_module(&self) -> TokenStream2 {
        let fn_ident = &self.fn_ident;
        let resolver = &self.resolver_path;
        let is_resolved = quote::format_ident!("nvn_internal_{}_is_resolved", fn_ident);
        let func_ptr = quote::format_ident!("nvn_internal_{}_func_ptr", fn_ident);
        let c_str_ident = syn::LitStr::new(format!("{}\0", fn_ident.to_string()).as_str(), Span::call_site());
        quote!(
            #[allow(non_snake_case)]
            mod #fn_ident {
                #[allow(unused_unsafe)]
                pub fn resolve() {
                    unsafe {
                        if !super::#is_resolved {
                            let (func_ptr, proper_resolve) = super::#resolver(#c_str_ident);
                            super::#func_ptr = func_ptr as _;
                            super::#is_resolved = !func_ptr.is_null() && proper_resolve;
                        }
                    }
                }
            }
        )
    }

    pub fn generate_callable(&self) -> TokenStream2 {
        let fn_ident = &self.fn_ident;
        let owner_path = &self.owner_path;
        let return_tokens = &self.return_tokens;
        let owner_arg = if self.is_const {
            quote!(this_self: *const #owner_path)
        } else {
            quote!(this_self: *mut #owner_path)
        };
        let is_resolved = quote::format_ident!("nvn_internal_{}_is_resolved", fn_ident);
        let func_ptr = quote::format_ident!("nvn_internal_{}_func_ptr", fn_ident);
        let arg = self.args.iter().map(|x| remove_mut(x));
        let arg2 = arg.clone();
        let arg3 = arg.clone();
        let arg_names = self.args.iter().filter_map(|x| {
            match x {
                syn::FnArg::Typed(pat_type) => {
                    match &*pat_type.pat {
                        syn::Pat::Ident(syn::PatIdent { ident, .. }) => Some(ident),
                        _ => None
                    }
                },
                _ => None
            }
        });
        quote!(
            #[allow(non_uppercase_globals)]
            static mut #is_resolved: bool = false;
            static mut #func_ptr: *const extern "C" fn(#owner_arg, #(#arg),*) = 0 as _;
            #[allow(non_snake_case)]
            fn #fn_ident(#owner_arg, #(#arg2),*) #return_tokens {
                unsafe {
                    if !#is_resolved {
                        #fn_ident::resolve();
                    }
                    core::mem::transmute::<_, extern "C" fn(#owner_arg, #(#arg3),*) #return_tokens>(#func_ptr)(this_self, #(#arg_names),*)
                }
            }
        )
    }
}

fn generate_nvn_impl(info: &NVNProcInfo, usr_field: &syn::Field) -> TokenStream2 {
    let vis = &usr_field.vis;
    let ident = usr_field.ident.as_ref().unwrap();
    let callable_name = &info.fn_ident;
    let return_tokens = &info.return_tokens;
    let args = info.args.iter();
    let arg_names = info.args.iter().filter_map(|x| {
        match x {
            syn::FnArg::Typed(pat_type) => {
                match &*pat_type.pat {
                    syn::Pat::Ident(syn::PatIdent { ident, .. }) => Some(ident),
                    _ => None
                }
            },
            _ => None
        }
    });
    if info.is_const {
        quote!(
            #[inline(never)]
            #vis fn #ident(&self, #(#args),*) #return_tokens {
                #callable_name(self, #(#arg_names),*)
            }
        )
    } else {
        quote!(
            #[inline(never)]
            #vis fn #ident(&mut self, #(#args),*) #return_tokens {
                #callable_name(self, #(#arg_names),*)
            }
        )
    }
}

#[proc_macro_attribute]
pub fn nvn_struct(attrs: TokenStream, input: TokenStream) -> TokenStream {
    let usr_attrs = parse_macro_input!(attrs as NVNStructArgs);
    let input = parse_macro_input!(input as syn::ItemStruct);

    let fields = match &input.fields {
        syn::Fields::Named(fields) => &fields.named,
        _ => panic!("NVN Struct fields must all be named")
    };

    let usr_ident = input.ident.clone();
    let usr_vis = input.vis.clone();

    let opaque_size = usr_attrs.opaque_size.clone();

    let attrs = fields.iter().map(|field| (&field.attrs, field));
    let mut infos = Vec::new();
    let mut impls = Vec::new();
    for (attr_list, field) in attrs {
        for attr in attr_list.iter() {
            let args = attr.tokens.to_string();
            if let Some(args) = args.strip_prefix("(") {
                if let Some(args) = args.strip_suffix(")") {
                    match syn::parse_str::<syn::Signature>(args) {
                        Ok(custom_sig) => {
                            let info = NVNProcInfo {
                                args: custom_sig.inputs.iter().map(|x| x.clone()).collect(),
                                owner_path: input.ident.clone(),
                                fn_ident: custom_sig.ident,
                                return_tokens: custom_sig.output.to_token_stream(),
                                is_const: custom_sig.constness.is_some(),
                                resolver_path: usr_attrs.resolver.clone()
                            };
                            impls.push(generate_nvn_impl(&info, field));
                            infos.push(info);
                        },
                        Err(error) => {
                            panic!("{}", error);
                        }
                    }
                }
            }
        }
    }

    let resolver_modules = infos.iter().map(|x| x.generate_resolver_module());
    let resolver_module_names = infos.iter().map(|x| x.fn_ident.clone());
    let callables = infos.iter().map(|x| x.generate_callable());
    let impls = impls.iter();

    let new_struct = quote!(
        #[repr(C)]
        #usr_vis struct #usr_ident {
            _opaque: [u8; #opaque_size]
        }
    );
    
    quote!(
        #(
            #resolver_modules
        )*

        #(
            #callables
        )*

        #new_struct 

        impl #usr_ident {
            pub const fn new() -> Self {
                Self {
                    _opaque: [0; #opaque_size]
                }
            }

            pub fn resolve() {
                #(
                    #resolver_module_names::resolve();
                )*
            }

            #(
                #impls
            )*
        }

        impl ::core::default::Default for #usr_ident {
            fn default() -> Self {
                Self::new()
            }
        }
    ).into()
}