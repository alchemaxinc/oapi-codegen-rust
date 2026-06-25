//! Compile-checks every *supported* generated output and exercises a few
//! representative types at runtime.
//!
//! Each module `include!`s a generated file so the test crate fails to build if
//! any emitted code stops compiling against its real dependencies (serde,
//! chrono, uuid, serde_json, axum). The module set is kept in lock-step with the
//! coverage matrix by `generated_outputs_are_compile_checked` in
//! `tests/coverage.rs`.
//!
//! The generated modules live under `mod generated`, which carries the only lint
//! exceptions in this file: emitted code is ordinary idiomatic Rust (tail
//! expressions, plus types these tests never construct), and the workspace's
//! `implicit_return`/`dead_code` rules are about first-party source, not
//! generated output. The handwritten tests below are linted normally.

#[allow(dead_code, clippy::implicit_return)]
mod generated {
    pub mod allof_merge {
        include!("generated/allof_merge.rs");
    }
    pub mod anyof_untagged {
        include!("generated/anyof_untagged.rs");
    }
    pub mod array_types {
        include!("generated/array_types.rs");
    }
    pub mod ext_x_rust_type {
        include!("generated/ext_x_rust_type.rs");
    }
    pub mod freeform_any {
        include!("generated/freeform_any.rs");
    }
    pub mod integer_formats {
        include!("generated/integer_formats.rs");
    }
    pub mod map_alias {
        include!("generated/map_alias.rs");
    }
    pub mod metadata_docs {
        include!("generated/metadata_docs.rs");
    }
    pub mod nullable {
        include!("generated/nullable.rs");
    }
    pub mod number_formats {
        include!("generated/number_formats.rs");
    }
    pub mod object_additional_properties {
        include!("generated/object_additional_properties.rs");
    }
    pub mod object_nested_inline {
        include!("generated/object_nested_inline.rs");
    }
    pub mod object_optional_required {
        include!("generated/object_optional_required.rs");
    }
    pub mod oneof_discriminator {
        include!("generated/oneof_discriminator.rs");
    }
    pub mod oneof_untagged {
        include!("generated/oneof_untagged.rs");
    }
    pub mod primitive_scalars {
        include!("generated/primitive_scalars.rs");
    }
    pub mod ref_local {
        include!("generated/ref_local.rs");
    }
    pub mod string_enum {
        include!("generated/string_enum.rs");
    }
    pub mod string_formats {
        include!("generated/string_formats.rs");
    }
    pub mod server_petstore {
        include!("generated/server_petstore.rs");
    }
}

#[test]
fn untagged_enum_round_trips_through_serde() {
    use generated::oneof_discriminator;

    let payment = oneof_discriminator::Payment::Card(oneof_discriminator::Card {
        number: "4111".to_owned(),
    });
    let json = serde_json::to_string(&payment).expect("serialize");
    let back: oneof_discriminator::Payment = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(payment, back);
}

#[test]
fn optional_field_is_skipped_when_none() {
    use generated::object_optional_required;

    let profile = object_optional_required::Profile {
        id: "u1".to_owned(),
        nickname: None,
    };
    let json = serde_json::to_string(&profile).expect("serialize");
    assert_eq!(json, r#"{"id":"u1"}"#);
}

#[test]
fn generated_server_trait_implements_and_routes() {
    use generated::server_petstore;
    use server_petstore::Api;
    use server_petstore::CreatePetResponse;
    use server_petstore::DeletePetResponse;
    use server_petstore::Error;
    use server_petstore::GetPetResponse;
    use server_petstore::ListPetsResponse;
    use server_petstore::NewPet;
    use server_petstore::Pet;

    #[derive(Clone)]
    struct Service;

    impl Api for Service {
        async fn list_pets(&self) -> ListPetsResponse {
            return ListPetsResponse::Ok(Vec::new());
        }

        async fn create_pet(&self, body: NewPet) -> CreatePetResponse {
            return CreatePetResponse::Created(Pet {
                id: 1,
                name: body.name,
                tag: body.tag,
            });
        }

        async fn get_pet(&self, id: i64) -> GetPetResponse {
            return GetPetResponse::NotFound(Error {
                message: format!("no pet {id}"),
            });
        }

        async fn delete_pet(&self, _id: i64) -> DeletePetResponse {
            return DeletePetResponse::NoContent;
        }
    }

    // Building the router proves the native `async fn` trait, the response
    // enums' `IntoResponse`, and the handler wiring all type-check together.
    let _router: axum::Router = server_petstore::router(Service);
}
