//! What `#[non_exhaustive]` costs and what it buys a downstream crate.
//!
//! The attribute has no effect inside the crate that defines it, so a unit test
//! cannot show its behaviour. These cases live in a separate crate, which is
//! what a real consumer is.
//!
//! The rule under test: the IR enums carry the attribute, the IR structs do
//! not. An enum stays open for a later variant, and a struct stays open for a
//! caller who builds an IR by hand.

use oapi_codegen::emit::emit_module;
use oapi_codegen::ir::Alias;
use oapi_codegen::ir::Item;
use oapi_codegen::ir::Module;
use oapi_codegen::ir::RustType;
use oapi_codegen::naming::Case;
use oapi_codegen::naming::to_ident;

/// `emit_module` is public and takes an IR, so a caller must be able to build
/// one. `#[non_exhaustive]` on these structs would stop every struct expression
/// outside the crate, even one with `..Default::default()`, and would leave the
/// function with no reachable input.
#[test]
fn a_consumer_can_build_an_ir_by_hand_and_emit_it() {
    let module = Module {
        items: vec![Item::Alias(Alias {
            name: to_ident("UserId", Case::Pascal),
            doc: None,
            deprecated: None,
            ty: RustType::U64,
        })],
    };

    let code = emit_module(&module, None).expect("a hand-built module should emit");

    assert!(code.contains("UserId"), "the alias name should reach the output");
}

/// The attribute leaves variant construction alone. Only an exhaustive `match`
/// changes, and a catch-all arm answers it, so a new variant lands without a
/// break here.
#[test]
fn a_consumer_matches_an_ir_enum_through_a_catch_all_arm() {
    let label = match RustType::U32 {
        RustType::U32 => "u32",
        _ => "another type",
    };

    assert_eq!(label, "u32");
}
