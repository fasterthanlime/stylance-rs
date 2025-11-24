use std::{env, path::Path};

use anyhow::Context as _;
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{quote, quote_spanned};
use syn::{parse_macro_input, LitStr};

fn try_import_style_classes_with_path(
    config_path: &Path,
    base_path: &Path,
    file_path: &Path,
    identifier_span: Span,
) -> anyhow::Result<TokenStream> {
    let config = stylance_core::load_config(config_path)?;
    let (_, classes) = stylance_core::get_classes(base_path, file_path, &config)?;

    let binding = file_path.canonicalize().unwrap();
    let full_path = binding.to_string_lossy();

    let identifiers = classes
        .iter()
        .map(|class| Ident::new(&class.original_name.replace('-', "_"), identifier_span))
        .collect::<Vec<_>>();

    let output_fields = classes.iter().zip(identifiers).map(|(class, class_ident)| {
        let class_str = &class.hashed_name;
        quote_spanned!(identifier_span =>
            #[allow(non_upper_case_globals)]
            pub const #class_ident: &str = #class_str;
        )
    });

    Ok(quote! {
        const _ : &[u8] = include_bytes!(#full_path);
        #(#output_fields )*
    }
    .into())
}

fn try_import_style_classes(input: &LitStr) -> anyhow::Result<TokenStream> {
    let manifest_dir_env =
        env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR env var not found")?;
    let manifest_path = Path::new(&manifest_dir_env);
    let file_path = manifest_path.join(Path::new(&input.value()));

    try_import_style_classes_with_path(manifest_path, manifest_path, &file_path, input.span())
}

#[proc_macro]
pub fn import_style_classes(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);

    match try_import_style_classes(&input) {
        Ok(ts) => ts,
        Err(err) => syn::Error::new_spanned(&input, err.to_string())
            .to_compile_error()
            .into(),
    }
}

fn try_import_style_classes_rel(input: &LitStr) -> anyhow::Result<TokenStream> {
    let manifest_dir_env =
        env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR env var not found")?;
    let manifest_path = Path::new(&manifest_dir_env);

    let Some(source_path) = input.span().unwrap().local_file() else {
        // rust-analyzer returns None here, so we check for that specifically.
        // For actual builds, we want to error if local_file() is unavailable.
        if std::env::var("RA_RUSTC_WRAPPER").is_ok()
            || std::env::var("RUST_ANALYZER").is_ok()
            || std::env::var("CARGO").map(|v| v.contains("rust-analyzer")).unwrap_or(false)
        {
            // Rust analyzer - bail silently
            return Ok(TokenStream::new());
        }
        // Real build - this shouldn't happen, error out
        anyhow::bail!(
            "import_style! could not determine source file location. \
             CARGO_MANIFEST_DIR={:?}, span location unavailable. \
             This may indicate a build system issue.",
            manifest_dir_env
        );
    };

    let css_path = source_path
        .parent()
        .expect("Macro source path should have a parent dir")
        .join(input.value());

    // In build systems like Buck2, sources may be copied to a different location
    // (e.g., buck-out/.../__srcs/src/foo.rs). In this case, the CSS file won't be under
    // CARGO_MANIFEST_DIR. We detect this and find a suitable base path by looking for
    // a directory named "__srcs" in the css_path ancestors, which is Buck2's convention
    // for the source root.
    //
    // We use the original manifest_path for config loading (Cargo.toml), but the
    // effective base path for CSS file resolution and hash computation.
    let effective_base_path = if let Ok(canonical_css) = css_path.canonicalize() {
        if let Ok(canonical_manifest) = manifest_path.canonicalize() {
            if canonical_css.starts_with(&canonical_manifest) {
                // CSS file is under manifest_path, use it directly
                manifest_path.to_path_buf()
            } else {
                // CSS file is outside manifest_path (e.g., Buck2 build)
                // Look for __srcs directory in ancestors as the effective root
                canonical_css
                    .ancestors()
                    .find(|p| p.file_name().map(|n| n == "__srcs").unwrap_or(false))
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| manifest_path.to_path_buf())
            }
        } else {
            manifest_path.to_path_buf()
        }
    } else {
        manifest_path.to_path_buf()
    };

    try_import_style_classes_with_path(manifest_path, &effective_base_path, &css_path, input.span())
}

#[proc_macro]
pub fn import_style_classes_rel(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);

    match try_import_style_classes_rel(&input) {
        Ok(ts) => ts,
        Err(err) => syn::Error::new_spanned(&input, err.to_string())
            .to_compile_error()
            .into(),
    }
}
