use std::ops::Deref;

use crate::anonymonize_lifetimes;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{
    parse::{Parse, ParseStream},
    spanned::Spanned,
    FnArg, Pat, Token,
};

/** Resolves a `pg_extern` argument or return `syn::Type` into metadata

Returns `(resolved_ty, optional, variadic, default, composite_type)`.

It resolves the following macros:

* `pgx::default!()`
* `pgx::composite_type!()`
*/
pub(crate) fn resolve_ty(
    ty: syn::Type,
) -> syn::Result<(
    syn::Type,
    bool,
    bool,
    Option<String>,
    Option<syn::Expr>,
)> {
    // There are four steps:
    // * Anonymize any lifetimes
    // * Resolve the `default!()` macro
    // * Resolve `composite_type!()`
    // * Resolving any flags for that resolved type so we can not have to do this later.

    // Anonymize lifetimes, so the SQL resolver isn't dealing with that.
    let ty = {
        let mut ty = ty;
        anonymonize_lifetimes(&mut ty);
        ty
    };

    // Resolve any `default` macro
    // We do this first as it's **always** in the first position. It's not valid deeper in the type.
    let (ty, default) = match ty.clone() {
        // default!(..)
        // composite_type!(..)
        syn::Type::Macro(macro_pat) => {
            let mac = &macro_pat.mac;
            let archetype = mac.path.segments.last().expect("No last segment");
            match archetype.ident.to_string().as_str() {
                "default" => {
                    let (maybe_resolved_ty, default) = handle_default_macro(mac)?;
                    (maybe_resolved_ty, default)
                }
                _ => (syn::Type::Macro(macro_pat), None),
            }
        }
        original => (original, None),
    };

    // Now, resolve any `composite_type` macro
    let (ty, sql) = match ty {
        // composite_type!(..)
        syn::Type::Macro(macro_pat) => {
            let mac = &macro_pat.mac;
            let archetype = mac.path.segments.last().expect("No last segment");
            match archetype.ident.to_string().as_str() {
                "default" => {
                    return Err(syn::Error::new(
                        mac.span(),
                        "default!(default!()) not supported, use it only once",
                    ))?
                }
                "composite_type" => {
                    let sql = Some(
                        handle_composite_type_macro(&mac)?,
                    );
                    let ty = syn::parse_quote! {
                        ::pgx::PgHeapTuple<'_, ::pgx::AllocatedByRust>
                    };
                    (ty, sql)
                }
                _ => (syn::Type::Macro(macro_pat), None),
            }
        }
        syn::Type::Path(path) => {
            let segments = path.path.clone();
            let last = segments.segments.last().ok_or(syn::Error::new(
                path.span(),
                "Could not read last segment of path",
            ))?;

            match last.ident.to_string().as_str() {
                // Option<composite_type!(..)>
                // Option<Vec<composite_type!(..)>>
                // Option<Vec<Option<composite_type!(..)>>>
                // Option<VariadicArray<composite_type!(..)>>
                // Option<VariadicArray<Option<composite_type!(..)>>>
                "Option" => resolve_option_inner(
                    path,
                    last.arguments.clone(),
                )?,
                // Vec<composite_type!(..)>
                // Vec<Option<composite_type!(..)>>
                "Vec" => {
                    resolve_vec_inner(path, last.arguments.clone())?
                }
                // VariadicArray<composite_type!(..)>
                // VariadicArray<Option<composite_type!(..)>>
                "VariadicArray" => resolve_variadic_array_inner(
                    path,
                    last.arguments.clone(),
                )?,
                // Array<composite_type!(..)>
                // Array<Option<composite_type!(..)>>
                "Array" => resolve_array_inner(
                    path,
                    last.arguments.clone(),
                )?,
                _ => (syn::Type::Path(path), None),
            }
        }
        original => (original, None),
    };

    // In this second setp, we go look at the resolved type and determine if it is a variadic, optional, etc.
    let (ty, variadic, optional) = match ty {
        syn::Type::Path(type_path) => {
            let path = &type_path.path;
            let last_segment = path.segments.last().ok_or(syn::Error::new(
                path.span(),
                "No last segment found while scanning path",
            ))?;
            let ident_string = last_segment.ident.to_string();
            match ident_string.as_str() {
                "Option" => {
                    // Option<VariadicArray<T>>
                    match &last_segment.arguments {
                        syn::PathArguments::AngleBracketed(angle_bracketed) => {
                            match angle_bracketed.args.first().ok_or(syn::Error::new(
                                angle_bracketed.span(),
                                "No inner arg for Option<T> found",
                            ))? {
                                syn::GenericArgument::Type(ty) => {
                                    match ty {
                                        // Option<VariadicArray<T>>
                                        syn::Type::Path(ref inner_type_path) => {
                                            let path = &inner_type_path.path;
                                            let last_segment =
                                                path.segments.last().ok_or(syn::Error::new(
                                                    path.span(),
                                                    "No last segment found while scanning path",
                                                ))?;
                                            let ident_string = last_segment.ident.to_string();
                                            match ident_string.as_str() {
                                                // Option<VariadicArray<T>>
                                                "VariadicArray" => {
                                                    (syn::Type::Path(type_path), true, true)
                                                }
                                                _ => (syn::Type::Path(type_path), false, true),
                                            }
                                        }
                                        // Option<T>
                                        _ => (syn::Type::Path(type_path), false, true),
                                    }
                                }
                                // Option<T>
                                _ => (syn::Type::Path(type_path), false, true),
                            }
                        }
                        // Option<T>
                        _ => (syn::Type::Path(type_path), false, true),
                    }
                }
                // VariadicArray<T>
                "VariadicArray" => (syn::Type::Path(type_path), true, false),
                // T
                _ => (syn::Type::Path(type_path), false, false),
            }
        }
        original => (original, false, false),
    };

    Ok((ty, optional, variadic, default, sql))
}

fn resolve_vec_inner(
    original: syn::TypePath,
    arguments: syn::PathArguments,
) -> syn::Result<(syn::Type, Option<syn::Expr>)> {
    match arguments {
        syn::PathArguments::AngleBracketed(path_arg) => match path_arg.args.first() {
            Some(syn::GenericArgument::Type(ty)) => match ty.clone() {
                syn::Type::Macro(macro_pat) => {
                    let mac = &macro_pat.mac;
                    let archetype = mac.path.segments.last().expect("No last segment");
                    match archetype.ident.to_string().as_str() {
                        "default" => {
                            return Err(syn::Error::new(mac.span(), "`Vec<default!(T, default)>` not supported, choose `default!(Vec<T>, ident)` instead"))?;
                        }
                        "composite_type" => {
                            let sql = Some(handle_composite_type_macro(mac)?);
                            let ty = syn::parse_quote! {
                                Vec<::pgx::PgHeapTuple<'_, ::pgx::AllocatedByRust>>
                            };
                            Ok((ty, sql))
                        }
                        _ => Ok((syn::Type::Path(original), None)),
                    }
                }
                syn::Type::Path(arg_type_path) => {
                    let last = arg_type_path.path.segments.last().ok_or(syn::Error::new(
                        arg_type_path.span(),
                        "No last segment in type path",
                    ))?;
                    match last.ident.to_string().as_str() {
                        "Option" => {
                            resolve_option_inner(original, last.arguments.clone())
                        }
                        _ => Ok((syn::Type::Path(original), None)),
                    }
                }
                _ => Ok((syn::Type::Path(original), None)),
            },
            _ => Ok((syn::Type::Path(original), None)),
        },
        _ => Ok((syn::Type::Path(original), None)),
    }
}

fn resolve_variadic_array_inner(
    original: syn::TypePath,
    arguments: syn::PathArguments,
) -> syn::Result<(syn::Type, Option<syn::Expr>)> {
    match arguments {
        syn::PathArguments::AngleBracketed(path_arg) => match path_arg.args.first() {
            Some(syn::GenericArgument::Type(ty)) => match ty.clone() {
                syn::Type::Macro(macro_pat) => {
                    let mac = &macro_pat.mac;
                    let archetype = mac.path.segments.last().expect("No last segment");
                    match archetype.ident.to_string().as_str() {
                        "default" => {
                            return Err(syn::Error::new(mac.span(), "`VariadicArray<default!(T, default)>` not supported, choose `default!(VariadicArray<T>, ident)` instead"))?;
                        }
                        "composite_type" => {
                            let sql = Some(handle_composite_type_macro(mac)?);
                            let ty = syn::parse_quote! {
                                ::pgx::VariadicArray<::pgx::PgHeapTuple<'_, ::pgx::AllocatedByRust>>
                            };
                            Ok((ty, sql))
                        }
                        _ => Ok((syn::Type::Path(original), None)),
                    }
                }
                syn::Type::Path(arg_type_path) => {
                    let last = arg_type_path.path.segments.last().ok_or(syn::Error::new(
                        arg_type_path.span(),
                        "No last segment in type path",
                    ))?;
                    match last.ident.to_string().as_str() {
                        "Option" => {
                            resolve_option_inner(original, last.arguments.clone())
                        }
                        _ => Ok((syn::Type::Path(original), None)),
                    }
                }
                _ => Ok((syn::Type::Path(original), None)),
            },
            _ => Ok((syn::Type::Path(original), None)),
        },
        _ => Ok((syn::Type::Path(original), None)),
    }
}

fn resolve_array_inner(
    original: syn::TypePath,
    arguments: syn::PathArguments,
) -> syn::Result<(syn::Type, Option<syn::Expr>)> {
    match arguments {
        syn::PathArguments::AngleBracketed(path_arg) => match path_arg.args.first() {
            Some(syn::GenericArgument::Type(ty)) => match ty.clone() {
                syn::Type::Macro(macro_pat) => {
                    let mac = &macro_pat.mac;
                    let archetype = mac.path.segments.last().expect("No last segment");
                    match archetype.ident.to_string().as_str() {
                        "default" => {
                            return Err(syn::Error::new(mac.span(), "`VariadicArray<default!(T, default)>` not supported, choose `default!(VariadicArray<T>, ident)` instead"))?;
                        }
                        "composite_type" => {
                            let sql = Some(handle_composite_type_macro(mac)?);
                            let ty = syn::parse_quote! {
                                ::pgx::Array<::pgx::PgHeapTuple<'_, ::pgx::AllocatedByRust>>
                            };
                            Ok((ty, sql))
                        }
                        _ => Ok((syn::Type::Path(original), None)),
                    }
                }
                syn::Type::Path(arg_type_path) => {
                    let last = arg_type_path.path.segments.last().ok_or(syn::Error::new(
                        arg_type_path.span(),
                        "No last segment in type path",
                    ))?;
                    match last.ident.to_string().as_str() {
                        "Option" => {
                            resolve_option_inner(original, last.arguments.clone())
                        }
                        _ => Ok((syn::Type::Path(original), None)),
                    }
                }
                _ => Ok((syn::Type::Path(original), None)),
            },
            _ => Ok((syn::Type::Path(original), None)),
        },
        _ => Ok((syn::Type::Path(original), None)),
    }
}

fn resolve_option_inner(
    original: syn::TypePath,
    arguments: syn::PathArguments,
) -> syn::Result<(syn::Type, Option<syn::Expr>)> {
    match arguments {
        syn::PathArguments::AngleBracketed(path_arg) => match path_arg.args.first() {
            Some(syn::GenericArgument::Type(ty)) => {
                match ty.clone() {
                    syn::Type::Macro(macro_pat) => {
                        let mac = &macro_pat.mac;
                        let archetype = mac.path.segments.last().expect("No last segment");
                        match archetype.ident.to_string().as_str() {
                            // Option<composite_type!(..)>
                            "composite_type" => {
                                let sql = Some(handle_composite_type_macro(mac)?);
                                let ty = syn::parse_quote! {
                                    Option<::pgx::PgHeapTuple<'_, ::pgx::AllocatedByRust>>
                                };
                                Ok((ty, sql))
                            },
                            // Option<default!(composite_type!(..))> isn't valid. If the user wanted the default to be `NULL` they just don't need a default.
                            "default" => return Err(syn::Error::new(mac.span(), "`Option<default!(T, \"my_default\")>` not supported, choose `Option<T>` for a default of `NULL`, or `default!(T, default)` for a non-NULL default"))?,
                            _ => Ok((syn::Type::Path(original), None)),
                        }
                    }
                    syn::Type::Path(arg_type_path) => {
                        let last = arg_type_path.path.segments.last().ok_or(syn::Error::new(
                            arg_type_path.span(),
                            "No last segment in type path",
                        ))?;
                        match last.ident.to_string().as_str() {
                            // Option<Vec<composite_type!(..)>>
                            // Option<Vec<Option<composite_type!(..)>>>
                            "Vec" => {
                                resolve_vec_inner(original, last.arguments.clone())
                            },
                            // Option<VariadicArray<composite_type!(..)>>
                            // Option<VariadicArray<Option<composite_type!(..)>>>
                            "VariadicArray" => {
                                resolve_variadic_array_inner(
                                    original,
                                    last.arguments.clone(),
                                )
                            },
                            // Option<Array<composite_type!(..)>>
                            // Option<Array<Option<composite_type!(..)>>>
                            "Array" => {
                                resolve_array_inner(
                                    original,
                                    last.arguments.clone(),
                                )
                            },
                            // Option<..>
                            _ => Ok((syn::Type::Path(original), None)),
                        }
                    }
                    _ => Ok((syn::Type::Path(original), None)),
                }
            }
            _ => Ok((syn::Type::Path(original), None)),
        },
        _ => Ok((syn::Type::Path(original), None)),
    }
}


fn handle_composite_type_macro(mac: &syn::Macro) -> syn::Result<syn::Expr> {
    let out: syn::Expr = mac.parse_body()?;
    Ok(out)
}

fn handle_default_macro(mac: &syn::Macro) -> syn::Result<(syn::Type, Option<String>)> {
    let out: DefaultMacro = mac.parse_body()?;
    let true_ty = out.ty;
    match out.expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(def),
            ..
        }) => {
            let value = def.value();
            Ok((true_ty, Some(value)))
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Float(def),
            ..
        }) => {
            let value = def.base10_digits();
            Ok((true_ty, Some(value.to_string())))
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(def),
            ..
        }) => {
            let value = def.base10_digits();
            Ok((true_ty, Some(value.to_string())))
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(def),
            ..
        }) => {
            let value = def.value();
            Ok((true_ty, Some(value.to_string())))
        }
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            ref expr,
            ..
        }) => match &**expr {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(def),
                ..
            }) => {
                let value = def.base10_digits();
                Ok((true_ty, Some("-".to_owned() + value)))
            }
            _ => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "Unrecognized UnaryExpr in `default!()` macro, got: {:?}",
                        out.expr
                    ),
                ))
            }
        },
        syn::Expr::Type(syn::ExprType { ref ty, .. }) => match ty.deref() {
            syn::Type::Path(syn::TypePath {
                path: syn::Path { segments, .. },
                ..
            }) => {
                let last = segments.last().expect("No last segment");
                let last_string = last.ident.to_string();
                if last_string.as_str() == "NULL" {
                    Ok((true_ty, Some(last_string)))
                } else {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!(
                            "Unable to parse default value of `default!()` macro, got: {:?}",
                            out.expr
                        ),
                    ));
                }
            }
            _ => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "Unable to parse default value of `default!()` macro, got: {:?}",
                        out.expr
                    ),
                ))
            }
        },
        syn::Expr::Path(syn::ExprPath {
            path: syn::Path { ref segments, .. },
            ..
        }) => {
            let last = segments.last().expect("No last segment");
            let last_string = last.ident.to_string();
            if last_string.as_str() == "NULL" {
                Ok((true_ty, Some(last_string)))
            } else {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "Unable to parse default value of `default!()` macro, got: {:?}",
                        out.expr
                    ),
                ));
            }
        }
        _ => {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "Unable to parse default value of `default!()` macro, got: {:?}",
                    out.expr
                ),
            ))
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DefaultMacro {
    ty: syn::Type,
    pub(crate) expr: syn::Expr,
}

impl Parse for DefaultMacro {
    fn parse(input: ParseStream) -> Result<Self, syn::Error> {
        let ty = input.parse()?;
        let _comma: Token![,] = input.parse()?;
        let expr = input.parse()?;
        Ok(Self { ty, expr })
    }
}
