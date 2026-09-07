//! Emitting the [`crate::ir`] as a module tree.
//!
//! The root file at the configured output path declares each child module with
//! an explicit `#[path]` and re-exports it, so every generated name keeps the
//! position it had in the single-file layout. The explicit path matters: a root
//! mounted with `#[path = "generated/restapi.rs"] mod restapi;` resolves a bare
//! `mod models;` beside *itself* rather than under a `restapi/` directory, so
//! only the written-out path places the children where they are. That path also
//! resolves correctly when the root is a plain `mod restapi;` in `src/`, which
//! lets one emitted tree serve both mounting styles.
//!
//! No `mod.rs` is written anywhere. A directory module beside a file module of
//! the same name is the `E0761` ambiguity, and the explicit `#[path]` removes
//! any need for one.

use proc_macro2::Ident;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;

use crate::emit::HEADER;
use crate::emit::Targets;
use crate::emit::axum;
use crate::emit::operation;
use crate::emit::render_body;
use crate::emit::reqwest;
use crate::emit::usage;
use crate::error::Result;
use crate::ir::Module;
use crate::ir::ServerUrls;
use crate::ir::Service;
use crate::naming::RustIdent;
use crate::naming::operations::axum_handler_name;
use crate::package::GeneratedFile;
use crate::package::GeneratedPackage;

/// The module holding the component models.
const MODELS: &str = "models";
/// The module holding the server-URL constants and builders.
const SERVER_URLS: &str = "server_urls";
/// The module holding the per-operation input and response types.
const OPERATIONS: &str = "operations";
/// The module holding the axum server interface.
const SERVER: &str = "server";
/// The module holding the blocking `reqwest` client.
const CLIENT: &str = "client";

/// How many modules separate an operation's file from the package root.
const OPERATION_DEPTH: usize = 2;
/// How many modules separate a top-level module file from the package root.
const MODULE_DEPTH: usize = 1;

/// Emit the module tree for a run that lowered a service.
///
/// `stem` is the output file's stem, which names the companion directory the
/// children live in.
pub fn emit_package(
    module: &Module,
    service: &Service,
    server_urls: Option<&ServerUrls>,
    targets: Targets,
    stem: &str,
) -> Result<GeneratedPackage> {
    let derives = usage::model_derives(module, service, targets);
    let foreign = usage::foreign_resolver(module);
    let modules: Vec<OperationModule> = service
        .operations
        .iter()
        .map(|operation| return operation_module(&operation.name))
        .collect();

    let mut package = Builder::new(stem);

    let models = super::module_items(module, &derives, super::uses_nullable(module, Some(service)))?;
    let has_models = !models.is_empty();
    package.add(MODELS, models)?;
    package.add(SERVER_URLS, super::server_url_items(server_urls)?)?;

    let mut declarations = Vec::with_capacity(modules.len());
    for (operation, module) in service.operations.iter().zip(&modules) {
        let imports = imports(OPERATION_DEPTH, has_models, false, TokenStream::new());
        let items = operation::emit_operation_types(operation, targets, &foreign)?;
        package.add_child(OPERATIONS, &module.stem, imports, items)?;
        declarations.push(reexport(OPERATIONS, module));
    }
    package.add(OPERATIONS, block(declarations))?;

    if targets.server {
        let items = server_items(service, &modules, has_models, &mut package)?;
        package.add(SERVER, items)?;
    }
    if targets.client {
        let items = client_items(service, &modules, has_models, &mut package)?;
        package.add(CLIENT, items)?;
    }

    return package.finish();
}

/// Emit the server operation files and return the items `server.rs` holds.
fn server_items(
    service: &Service,
    modules: &[OperationModule],
    has_models: bool,
    package: &mut Builder,
) -> Result<Vec<TokenStream>> {
    let items = axum::server_items(service, axum::HandlerVisibility::Parent)?;
    let api = format_ident!("{}", axum::API_TRAIT_NAME);
    let mut declarations = Vec::with_capacity(modules.len());
    for ((entry, operation), module) in items.operations.into_iter().zip(&service.operations).zip(modules) {
        // The handler names the `Api` trait in its bound, and the router in the
        // parent module names the handler.
        let imports = imports(OPERATION_DEPTH, has_models, true, quote! { use super::#api; });
        let mut file = entry.extractors;
        file.push(entry.into_response);
        file.push(entry.handler);
        package.add_child(SERVER, &module.stem, imports, file)?;
        let declaration = declare(SERVER, module);
        let ident = &module.ident;
        let handler = axum_handler_name(&operation.name).to_token();
        declarations.push(quote! {
            #declaration
            use #ident::#handler;
        });
    }
    let mut file: Vec<TokenStream> = imports(MODULE_DEPTH, has_models, !modules.is_empty(), TokenStream::new())
        .into_iter()
        .collect();
    file.extend(block(declarations));
    file.push(items.api_trait);
    file.push(items.router);
    return Ok(file);
}

/// Emit the client operation files and return the items `client.rs` holds.
fn client_items(
    service: &Service,
    modules: &[OperationModule],
    has_models: bool,
    package: &mut Builder,
) -> Result<Vec<TokenStream>> {
    let items = reqwest::client_items(service)?;
    let client = format_ident!("{}", reqwest::CLIENT_STRUCT_NAME);
    let error = format_ident!("{}", reqwest::CLIENT_ERROR_NAME);
    let encode_set = format_ident!("{}", reqwest::ENCODE_SET_NAME);
    let mut declarations = Vec::with_capacity(modules.len());
    for ((method, operation), module) in items
        .operation_methods
        .into_iter()
        .zip(&service.operations)
        .zip(modules)
    {
        // An inherent `impl` applies crate-wide, so the method reaches callers
        // from its own module. Only the names it mentions have to be imported.
        let mut extra = quote! { use super::{#client, #error}; };
        if !operation.path_params.is_empty() {
            extra.extend(quote! { use super::#encode_set; });
        }
        let imports = imports(OPERATION_DEPTH, has_models, true, extra);
        let body = vec![quote! {
            impl #client {
                #method
            }
        }];
        package.add_child(CLIENT, &module.stem, imports, body)?;
        declarations.push(declare(CLIENT, module));
    }
    let shared = items.shared_methods;
    let mut file = block(declarations);
    file.push(items.error);
    file.extend(items.encode_set);
    file.push(items.client_struct);
    file.push(quote! {
        impl #client {
            #(#shared)*
        }
    });
    return Ok(file);
}

/// Collects the files of a package and the modules its root mounts.
struct Builder {
    /// The output file's stem, which names the companion directory.
    stem: String,
    /// The finished child files.
    files: Vec<GeneratedFile>,
    /// The top-level modules the root mounts, in emission order.
    mounted: Vec<&'static str>,
}

impl Builder {
    /// Start an empty package for the given output stem.
    fn new(stem: &str) -> Self {
        return Self {
            stem: stem.to_owned(),
            files: Vec::new(),
            mounted: Vec::new(),
        };
    }

    /// Add a top-level module, skipping it when it holds no item.
    ///
    /// An empty module would leave the root re-exporting a module with nothing
    /// public in it, which rustc rejects, so an absent module is the right
    /// answer for a run that produces none of its items.
    fn add(&mut self, name: &'static str, items: Vec<TokenStream>) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let path = format!("{}/{name}.rs", self.stem);
        self.files.push(GeneratedFile::new(path, render(&items)?));
        self.mounted.push(name);
        return Ok(());
    }

    /// Add one operation's file under a top-level module.
    fn add_child(
        &mut self,
        parent: &'static str,
        stem: &str,
        imports: Option<TokenStream>,
        items: Vec<TokenStream>,
    ) -> Result<()> {
        let file: Vec<TokenStream> = imports.into_iter().chain(items).collect();
        let path = format!("{}/{parent}/{stem}.rs", self.stem);
        self.files.push(GeneratedFile::new(path, render(&file)?));
        return Ok(());
    }

    /// Render the root facade and hand back the finished package.
    fn finish(mut self) -> Result<GeneratedPackage> {
        let mounts = self.mounted.iter().map(|name| {
            let ident = format_ident!("{name}");
            let path = format!("{}/{name}.rs", self.stem);
            return quote! {
                #[path = #path]
                mod #ident;
                pub use #ident::*;
            };
        });
        let root = render(&[quote! { #(#mounts)* }])?;
        self.files.sort_by(|left, right| return left.path().cmp(right.path()));
        return Ok(GeneratedPackage::new(root, self.files));
    }
}

/// Join related declarations into one rendered item, or nothing when there are
/// none.
///
/// Items are rendered a blank line apart, which reads well between types and
/// badly between a run of one-line `mod` declarations.
fn block(declarations: Vec<TokenStream>) -> Vec<TokenStream> {
    if declarations.is_empty() {
        return Vec::new();
    }
    return vec![quote! { #(#declarations)* }];
}

/// The file and module an operation gets.
///
/// The two are tracked together because they can differ. An operation named
/// `mod` needs the file stem `mod_`, since `mod.rs` beside `operations.rs` is
/// the `E0761` ambiguity, while its module item stays `r#mod`. The explicit
/// `#[path]` ties the two back together.
struct OperationModule {
    /// The file stem, without the `.rs`.
    stem: String,
    /// The identifier of the module item, raw when the name is a keyword.
    ident: Ident,
}

/// The file and module for the operation named `name`.
///
/// Operation names are unique `snake_case` identifiers, so the stems collide
/// with nothing on any filesystem, including a case-insensitive one.
fn operation_module(name: &RustIdent) -> OperationModule {
    let text = name.logical();
    let stem = if text == "mod" {
        format!("{text}_")
    } else {
        text.to_owned()
    };
    return OperationModule {
        stem,
        ident: name.to_token(),
    };
}

/// The declaration and re-export a parent module writes for one child.
fn reexport(parent: &'static str, module: &OperationModule) -> TokenStream {
    let declaration = declare(parent, module);
    let ident = &module.ident;
    return quote! {
        #declaration
        pub use #ident::*;
    };
}

/// The `#[path]`-carrying `mod` item a parent module writes for one child.
fn declare(parent: &'static str, module: &OperationModule) -> TokenStream {
    let path = format!("{parent}/{}.rs", module.stem);
    let ident = &module.ident;
    return quote! {
        #[path = #path]
        mod #ident;
    };
}

/// The `use` items a generated module needs, or nothing when it needs none.
///
/// `depth` is how many modules lie between the file and the package root. A file
/// imports every name it could refer to, whether or not it does: an operation
/// that names only primitives references no model, and the header's
/// `unused_imports` allowance covers that.
fn imports(depth: usize, models: bool, operations: bool, extra: TokenStream) -> Option<TokenStream> {
    if !models && !operations && extra.is_empty() {
        return None;
    }
    let root = root_path(depth);
    let models = models.then(|| {
        let ident = format_ident!("{MODELS}");
        return quote! { use #root #ident::*; };
    });
    let operations = operations.then(|| {
        let ident = format_ident!("{OPERATIONS}");
        return quote! { use #root #ident::*; };
    });
    return Some(quote! {
        #models
        #operations
        #extra
    });
}

/// The `super::` chain that reaches the package root from `depth` modules down.
fn root_path(depth: usize) -> TokenStream {
    let hops = std::iter::repeat_n(quote! { super:: }, depth);
    return quote! { #(#hops)* };
}

/// Render a file: the generated header, then the items one blank line apart.
fn render(items: &[TokenStream]) -> Result<String> {
    let mut out = String::from(HEADER);
    out.push_str(&render_body(items)?);
    return Ok(out);
}
