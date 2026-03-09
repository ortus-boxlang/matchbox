extern crate proc_macro;
use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{parse_macro_input, ItemFn, FnArg, Pat, ItemStruct, ItemImpl, ImplItem};

#[proc_macro_attribute]
pub fn matchbox_fn(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let name = &input.sig.ident;
    let wrapper_name = format_ident!("{}_wrapper", name);
    let vis = &input.vis;

    let mut arg_conversions = Vec::new();
    let mut call_args = Vec::new();

    for (i, arg) in input.sig.inputs.iter().enumerate() {
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                let arg_name = &pat_ident.ident;
                let arg_type = &pat_type.ty;
                
                let conversion = if quote!(#arg_type).to_string().contains("f64") {
                    quote! { let #arg_name = args[#i].as_number(); }
                } else if quote!(#arg_type).to_string().contains("i32") {
                    quote! { let #arg_name = args[#i].as_int(); }
                } else if quote!(#arg_type).to_string().contains("bool") {
                    quote! { let #arg_name = args[#i].as_bool(); }
                } else if quote!(#arg_type).to_string().contains("String") {
                    quote! { let #arg_name = vm.to_string(args[#i]); }
                } else {
                    quote! { let #arg_name = args[#i]; }
                };
                
                arg_conversions.push(conversion);
                call_args.push(quote!(#arg_name));
            }
        }
    }

    let arg_count = call_args.len();
    
    let expanded = quote! {
        #input

        #vis fn #wrapper_name(vm: &mut dyn matchbox_vm::types::BxVM, args: &[matchbox_vm::types::BxValue]) -> Result<matchbox_vm::types::BxValue, String> {
            if args.len() != #arg_count {
                return Err(format!("{} requires {} arguments, got {}", stringify!(#name), #arg_count, args.len()));
            }
            #(#arg_conversions)*
            let result = #name(#(#call_args),*);
            Ok(matchbox_vm::types::BxValue::new_number(result as f64))
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn matchbox_class(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;

    let expanded = quote! {
        #input

        impl matchbox_vm::types::BxNativeObject for #name {
            fn get_property(&self, _name: &str) -> matchbox_vm::types::BxValue {
                matchbox_vm::types::BxValue::new_null()
            }

            fn set_property(&mut self, _name: &str, _value: matchbox_vm::types::BxValue) {}

            fn call_method(&mut self, vm: &mut dyn matchbox_vm::types::BxVM, name: &str, args: &[matchbox_vm::types::BxValue]) -> Result<matchbox_vm::types::BxValue, String> {
                self.dispatch_method(vm, name, args)
            }
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn matchbox_methods(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;
    
    let mut dispatch_arms = Vec::new();

    for item in &input.items {
        if let ImplItem::Fn(method) = item {
            let name = &method.sig.ident;
            let name_str = name.to_string().to_lowercase();
            
            let mut arg_conversions = Vec::new();
            let mut call_args = Vec::new();
            let mut skip_first = false;

            for (i, arg) in method.sig.inputs.iter().enumerate() {
                if let FnArg::Receiver(_) = arg {
                    skip_first = true;
                    continue;
                }

                if let FnArg::Typed(pat_type) = arg {
                    if let Pat::Ident(pat_ident) = &*pat_type.pat {
                        let arg_name = &pat_ident.ident;
                        let arg_type = &pat_type.ty;
                        let arg_idx = if skip_first { i - 1 } else { i };

                        let conversion = if quote!(#arg_type).to_string().contains("f64") {
                            quote! { let #arg_name = args[#arg_idx].as_number(); }
                        } else if quote!(#arg_type).to_string().contains("i32") {
                            quote! { let #arg_name = args[#arg_idx].as_int(); }
                        } else if quote!(#arg_type).to_string().contains("String") {
                            quote! { let #arg_name = vm.to_string(args[#arg_idx]); }
                        } else {
                            quote! { let #arg_name = args[#arg_idx]; }
                        };
                        
                        arg_conversions.push(conversion);
                        call_args.push(quote!(#arg_name));
                    }
                }
            }

            let arg_count = call_args.len();

            dispatch_arms.push(quote! {
                #name_str => {
                    if args.len() != #arg_count {
                        return Err(format!("{} requires {} arguments, got {}", #name_str, #arg_count, args.len()));
                    }
                    #(#arg_conversions)*
                    let result = self.#name(#(#call_args),*);
                    // Check if result is already BxValue or needs wrapping
                    Ok(matchbox_vm::types::BxValue::new_number(result as f64))
                }
            });
        }
    }

    let expanded = quote! {
        #input

        impl #self_ty {
            fn dispatch_method(&mut self, vm: &mut dyn matchbox_vm::types::BxVM, name: &str, args: &[matchbox_vm::types::BxValue]) -> Result<matchbox_vm::types::BxValue, String> {
                match name.to_lowercase().as_str() {
                    #(#dispatch_arms)*
                    _ => Err(format!("Method {} not found", name)),
                }
            }
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn matchbox_module(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
