// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    fold::{fold_expr, Fold},
    parse::Parser,
    parse2, parse_macro_input,
    punctuated::Punctuated,
    BinOp, Expr, ExprMacro, Token, UnOp,
};

#[proc_macro_attribute]
pub fn init_static_initializers(_args: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as syn::ItemFn);

    let body = &input.block;
    input.block = syn::parse2(quote! {
        {
            // We have some lazily-initialized static state in the program. The initializers
            // alter the thread-local hash container state any time they create a new hash
            // container. Therefore, we need to ensure that these initializers are run in a
            // separate thread before the first test thread is launched. Otherwise, they would
            // run inside of the first test thread, but not subsequent ones.
            //
            // Note that none of this has any effect on process-level determinism. Without this
            // code, we can still get the same test results from two processes started with the
            // same seed.
            //
            // However, when using sim_test(check_determinism) or MSIM_TEST_CHECK_DETERMINISM=1,
            // we want the same test invocation to be deterministic when run twice
            // _in the same process_, so we need to take care of this. This will also
            // be very important for being able to reproduce a failure that occurs in the Nth
            // iteration of a multi-iteration test run.
            std::thread::spawn(|| {
                use sui_simulator::sui_framework::SystemPackage;
                ::sui_simulator::telemetry_subscribers::init_for_testing();
                ::sui_simulator::sui_framework::MoveStdlib::as_modules();
                ::sui_simulator::sui_framework::SuiFramework::as_modules();
                ::sui_simulator::sui_framework::SuiSystem::as_modules();
                ::sui_simulator::sui_types::gas::SuiGasStatus::new_unmetered();

                // For reasons I can't understand, LruCache causes divergent behavior the second
                // time one is constructed and inserted into, so construct one before the first
                // test run for determinism.
                let mut cache = ::sui_simulator::lru::LruCache::new(1.try_into().unwrap());
                cache.put(1, 1);

                {
                    // Initialize the static initializers here:
                    // https://github.com/move-language/move/blob/652badf6fd67e1d4cc2aa6dc69d63ad14083b673/language/tools/move-package/src/package_lock.rs#L12
                    use std::path::PathBuf;
                    use sui_simulator::sui_framework_build::compiled_package::{BuildConfig, SuiPackageHooks};
                    use sui_simulator::sui_framework::build_move_package;
                    use sui_simulator::tempfile::TempDir;
		    use sui_simulator::move_package::package_hooks::register_package_hooks;

		    move_package::package_hooks::register_package_hooks(Box::new(SuiPackageHooks {}));
                    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                    path.push("../../sui_programmability/examples/basics");
                    let mut build_config = BuildConfig::default();

                    build_config.config.install_dir = Some(TempDir::new().unwrap().into_path());
                    let _all_module_bytes = build_move_package(&path, build_config)
                        .unwrap()
                        .get_package_bytes(/* with_unpublished_deps */ false);
                }


                use ::sui_simulator::anemo_tower::callback::CallbackLayer;
                use ::sui_simulator::anemo_tower::trace::DefaultMakeSpan;
                use ::sui_simulator::anemo_tower::trace::DefaultOnFailure;
                use ::sui_simulator::anemo_tower::trace::TraceLayer;
                use ::sui_simulator::narwhal_network::metrics::MetricsMakeCallbackHandler;
                use ::sui_simulator::narwhal_network::metrics::NetworkMetrics;

                use std::sync::Arc;
                use ::sui_simulator::fastcrypto::traits::KeyPair;
                use rand::rngs::{StdRng, OsRng};
                use rand::SeedableRng;
                use ::sui_simulator::tower::ServiceBuilder;

                // anemo uses x509-parser, which has many lazy static variables. start a network to
                // initialize all that static state before the first test.
                let rt = ::sui_simulator::runtime::Runtime::new();
                rt.block_on(async move {
                    use ::sui_simulator::anemo::{Network, Request};

                    let make_network = |port: u16| {
                        let registry = prometheus::Registry::new();
                        let inbound_network_metrics =
                            NetworkMetrics::new("sui", "inbound", &registry);
                        let outbound_network_metrics =
                            NetworkMetrics::new("sui", "outbound", &registry);

                        let service = ServiceBuilder::new()
                            .layer(
                                TraceLayer::new_for_server_errors()
                                    .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                                    .on_failure(DefaultOnFailure::new().level(tracing::Level::WARN)),
                            )
                            .layer(CallbackLayer::new(MetricsMakeCallbackHandler::new(
                                Arc::new(inbound_network_metrics),
                                usize::MAX,
                            )))
                            .service(::sui_simulator::anemo::Router::new());

                        let outbound_layer = ServiceBuilder::new()
                            .layer(
                                TraceLayer::new_for_client_and_server_errors()
                                    .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                                    .on_failure(DefaultOnFailure::new().level(tracing::Level::WARN)),
                            )
                            .layer(CallbackLayer::new(MetricsMakeCallbackHandler::new(
                                Arc::new(outbound_network_metrics),
                                usize::MAX,
                            )))
                            .into_inner();


                        Network::bind(format!("127.0.0.1:{}", port))
                            .server_name("static-init-network")
                            .private_key(
                                ::sui_simulator::fastcrypto::ed25519::Ed25519KeyPair::generate(&mut StdRng::from_rng(OsRng).unwrap())
                                    .private()
                                    .0
                                    .to_bytes(),
                            )
                            .start(service)
                            .unwrap()
                    };
                    let n1 = make_network(80);
                    let n2 = make_network(81);

                    let _peer = n1.connect(n2.local_addr()).await.unwrap();
                });
            }).join().unwrap();

            #body
        }
    })
    .expect("Parsing failure");

    let result = quote! {
        #input
    };

    result.into()
}

/// The sui_test macro will invoke either `#[msim::test]` or `#[tokio::test]`,
/// depending on whether the simulator config var is enabled.
///
/// This should be used for tests that can meaningfully run in either environment.
#[proc_macro_attribute]
pub fn sui_test(args: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::ItemFn);
    let args = parse_macro_input!(args as syn::AttributeArgs);

    let header = if cfg!(msim) {
        quote! {
            #[::sui_simulator::sim_test(crate = "sui_simulator", #(#args)*)]
            #[::sui_macros::init_static_initializers]
        }
    } else {
        quote! {
            #[::tokio::test(#(#args)*)]
            // though this is not required for tokio, we do it to get logs as well.
            #[::sui_macros::init_static_initializers]
        }
    };

    let result = quote! {
        #header
        #input
    };

    result.into()
}

/// The sim_test macro will invoke `#[msim::test]` if the simulator config var is enabled.
///
/// Otherwise, it will emit an ignored test - if forcibly run, the ignored test will panic.
///
/// This macro must be used in order to pass any simulator-specific arguments, such as
/// `check_determinism`, which is not understood by tokio.
#[proc_macro_attribute]
pub fn sim_test(args: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::ItemFn);
    let args = parse_macro_input!(args as syn::AttributeArgs);

    let result = if cfg!(msim) {
        quote! {
            #[::sui_simulator::sim_test(crate = "sui_simulator", #(#args)*)]
            #[::sui_macros::init_static_initializers]
            #input
        }
    } else {
        let fn_name = &input.sig.ident;
        let sig = &input.sig;
        let body = &input.block;
        quote! {
            #[tokio::test]
            #sig {
                if std::env::var("SUI_SKIP_SIMTESTS").is_ok() {
                    println!("not running test {} in `cargo test`: SUI_SKIP_SIMTESTS is set", stringify!(#fn_name));

                    struct Ret;

                    impl From<Ret> for () {
                        fn from(_ret: Ret) -> Self {
                        }
                    }

                    impl<E> From<Ret> for Result<(), E> {
                        fn from(_ret: Ret) -> Self {
                            Ok(())
                        }
                    }

                    return Ret.into();
                }

                #body
            }
        }
    };

    result.into()
}

#[proc_macro]
pub fn checked_arithmetic(input: TokenStream) -> TokenStream {
    let input_file = CheckArithmetic.fold_file(parse_macro_input!(input));

    let output_items = input_file.items;

    let output = quote! {
        #(#output_items)*
    };

    TokenStream::from(output)
}

#[proc_macro_attribute]
pub fn with_checked_arithmetic(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_item = parse_macro_input!(item as syn::Item);

    match input_item {
        syn::Item::Fn(input_fn) => {
            let transformed_fn = CheckArithmetic.fold_item_fn(input_fn);
            TokenStream::from(quote! { #transformed_fn })
        }
        syn::Item::Impl(input_impl) => {
            let transformed_impl = CheckArithmetic.fold_item_impl(input_impl);
            TokenStream::from(quote! { #transformed_impl })
        }
        _ => {
            let error = syn::Error::new_spanned(item, "could not process macro contents");
            return Expr::Verbatim(error.to_compile_error());
        }
            panic!(
            "The with_checked_arithmetic attribute can only be applied to functions and impl blocks"
        ),
    }
}

struct CheckArithmetic;

impl Fold for CheckArithmetic {
    fn fold_expr(&mut self, expr: Expr) -> Expr {
        let expr = fold_expr(self, expr);
        let expr = match expr {
            Expr::Macro(expr_macro) => {
                let ExprMacro { mut attrs, mut mac } = expr_macro;

                if let Some(idx) = attrs
                    .iter()
                    .position(|attr| attr.path.is_ident("skip_checked_arithmetic"))
                {
                    // Skip processing macro because it is annotated with
                    // #[skip_checked_arithmetic]
                    attrs.remove(idx);
                    let expr_macro = Expr::Macro(ExprMacro { attrs, mac });
                    quote!(#expr_macro)
                } else {
                    // Parse the macro's contents as a comma-separated list of expressions.
                    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
                    let Ok(exprs) = parser.parse(mac.tokens.clone().into()) else {
                    let error = syn::Error::new_spanned(mac.tokens, "could not process macro contents");
                    return Expr::Verbatim(error.to_compile_error());
                };

                    // Fold each sub expression.
                    let folded_exprs = exprs
                        .into_iter()
                        .map(|expr| self.fold_expr(expr))
                        .collect::<Vec<_>>();

                    // Convert the folded expressions back into tokens and reconstruct the macro.
                    let mut folded_tokens = proc_macro2::TokenStream::new();
                    for (i, folded_expr) in folded_exprs.into_iter().enumerate() {
                        if i > 0 {
                            folded_tokens.extend(std::iter::once::<proc_macro2::TokenTree>(
                                proc_macro2::Punct::new(',', proc_macro2::Spacing::Alone).into(),
                            ));
                        }
                        folded_expr.to_tokens(&mut folded_tokens);
                    }

                    mac.tokens = folded_tokens;
                    let expr_macro = ExprMacro { attrs, mac };
                    quote!(#expr_macro)
                }
            }

            Expr::AssignOp(expr_assign_op) => {
                // Handle assignment operators with arithmetic here
                let op = &expr_assign_op.op;
                let lhs = &expr_assign_op.left;
                let rhs = &expr_assign_op.right;
                match op {
                    BinOp::AddEq(_) => {
                        quote!(#lhs = #lhs.checked_add(#rhs).expect("Overflow or underflow in addition assignment"))
                    }
                    BinOp::SubEq(_) => {
                        quote!(#lhs = #lhs.checked_sub(#rhs).expect("Overflow or underflow in subtraction assignment"))
                    }
                    BinOp::MulEq(_) => {
                        quote!(#lhs = #lhs.checked_mul(#rhs).expect("Overflow or underflow in multiplication assignment"))
                    }
                    BinOp::DivEq(_) => {
                        quote!(#lhs = #lhs.checked_div(#rhs).expect("Overflow or underflow in division assignment"))
                    }
                    BinOp::RemEq(_) => {
                        quote!(#lhs = #lhs.checked_rem(#rhs).expect("Overflow or underflow in remainder assignment"))
                    }
                    _ => quote!(#expr_assign_op),
                }
            }
            Expr::Binary(expr_binary) => {
                let op = &expr_binary.op;
                let lhs = &expr_binary.left;
                let rhs = &expr_binary.right;
                match op {
                    BinOp::Add(_) => {
                        quote!((#lhs).checked_add(#rhs).expect("Overflow or underflow in addition"))
                    }
                    BinOp::Sub(_) => {
                        quote!((#lhs).checked_sub(#rhs).expect("Overflow or underflow in subtraction"))
                    }
                    BinOp::Mul(_) => {
                        quote!((#lhs).checked_mul(#rhs).expect("Overflow or underflow in multiplication"))
                    }
                    BinOp::Div(_) => {
                        quote!((#lhs).checked_div(#rhs).expect("Overflow or underflow in division"))
                    }
                    BinOp::Rem(_) => {
                        quote!((#lhs).checked_rem(#rhs).expect("Overflow or underflow in remainder"))
                    }
                    _ => quote!(#expr_binary),
                }
            }
            Expr::Unary(expr_unary) => {
                let op = &expr_unary.op;
                let operand = &expr_unary.expr;
                match op {
                    UnOp::Neg(_) => {
                        quote!(#operand.checked_neg().expect("Overflow or underflow in negation"))
                    }
                    _ => quote!(#expr_unary),
                }
            }
            _ => quote!(#expr),
        };

        parse2(expr).unwrap()
    }
}
