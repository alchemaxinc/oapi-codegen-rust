// Stand-in for the models crate the `server_refs` and `server_xfile_refs`
// fixtures point their cross-file `$ref` bodies at through `import-mapping`.
//
// A real project generates this module from the referenced schema file. Here a
// minimal struct per referenced schema proves the emitted
// `crate::apimodel::CreateWidget` path resolves, and that the generated handler
// can decode it as a JSON body.
//
// This file is `include!`d rather than declared, because two crates need it at
// their own crate root: `tests/generated.rs` and the `msrv-check` crate. The
// emitted path is absolute (`crate::apimodel`), so each crate must hold its own
// copy of the module, and a copy written by hand in each place drifts as soon as
// a fixture references a new schema.
//
// The comments here are `//` and not `//!`. An `include!` expansion sits after the
// start of the module body, and an inner doc comment is only valid at that start.

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct CreateWidget {
    pub name: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct NewThing {
    pub name: String,
}
