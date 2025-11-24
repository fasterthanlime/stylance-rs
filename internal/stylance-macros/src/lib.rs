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

/// Find a directory containing Cargo.toml by walking up from the given path.
fn find_cargo_manifest_dir(start: &Path) -> Option<std::path::PathBuf> {
    start
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists())
        .map(|p| p.to_path_buf())
}

/// Find the effective source root for Buck2 builds by looking for __srcs directory.
fn find_buck2_source_root(path: &Path) -> Option<std::path::PathBuf> {
    path.ancestors()
        .find(|p| p.file_name().map(|n| n == "__srcs").unwrap_or(false))
        .map(|p| p.to_path_buf())
}

fn try_import_style_classes_rel(input: &LitStr) -> anyhow::Result<TokenStream> {
    let Some(source_path) = input.span().unwrap().local_file() else {
        // Real build - this shouldn't happen, error out
        anyhow::bail!(
            "import_style! could not determine source file location. \
             span location unavailable. This may indicate a build system issue."
        );
    };

    let css_path = source_path
        .parent()
        .expect("Macro source path should have a parent dir")
        .join(input.value());

    let canonical_css = css_path
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize CSS path: {:?}", css_path))?;

    // Try to find the manifest directory from CARGO_MANIFEST_DIR first (standard cargo builds),
    // then fall back to searching from the source file location (Buck2 and other build systems).
    let manifest_dir_env = env::var_os("CARGO_MANIFEST_DIR");

    let (config_path, base_path) = if let Some(ref manifest_dir) = manifest_dir_env {
        let manifest_path = Path::new(manifest_dir);
        if let Ok(canonical_manifest) = manifest_path.canonicalize() {
            if canonical_css.starts_with(&canonical_manifest) {
                // Standard cargo build: CSS file is under CARGO_MANIFEST_DIR
                (manifest_path.to_path_buf(), manifest_path.to_path_buf())
            } else {
                // Build system copied sources elsewhere (e.g., Buck2)
                // Use __srcs as base for hashing, but still try to load config from manifest
                let base = find_buck2_source_root(&canonical_css)
                    .unwrap_or_else(|| manifest_path.to_path_buf());
                (manifest_path.to_path_buf(), base)
            }
        } else {
            // CARGO_MANIFEST_DIR set but doesn't exist - find from source
            let dir = find_cargo_manifest_dir(&canonical_css)
                .context("Could not find Cargo.toml in any parent directory")?;
            (dir.clone(), dir)
        }
    } else {
        // No CARGO_MANIFEST_DIR - find everything from the source file location
        // First check if this is a Buck2 build with __srcs
        if let Some(srcs_root) = find_buck2_source_root(&canonical_css) {
            // Buck2 build: look for Cargo.toml starting from __srcs parent
            // (Buck2 puts sources in __srcs but Cargo.toml is typically in the parent)
            let config_dir = find_cargo_manifest_dir(srcs_root.parent().unwrap_or(&srcs_root))
                .or_else(|| find_cargo_manifest_dir(&canonical_css))
                .unwrap_or_else(|| srcs_root.clone());
            (config_dir, srcs_root)
        } else {
            // Regular build without CARGO_MANIFEST_DIR - find Cargo.toml from source
            let dir = find_cargo_manifest_dir(&canonical_css)
                .context("Could not find Cargo.toml in any parent directory")?;
            (dir.clone(), dir)
        }
    };

    try_import_style_classes_with_path(&config_path, &base_path, &css_path, input.span())
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
