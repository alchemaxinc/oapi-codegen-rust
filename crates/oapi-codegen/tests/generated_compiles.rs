//! Compile-checks every *supported* golden output and round-trips a
//! representative type through serde.
//!
//! Each `mod` `include!`s a generated golden so the test crate fails to build
//! if any emitted code stops compiling against its real dependencies (serde,
//! chrono, uuid, serde_json, axum). The set of modules below is kept in
//! lock-step with the coverage matrix by `supported_goldens_are_compile_checked`
//! in `tests/coverage.rs`.
#![allow(dead_code)]
// Generated code is idiomatic (tail expressions); this workspace's bespoke
// `implicit_return` lint does not apply to it.
#![allow(clippy::implicit_return)]

mod allof_merge {
    include!("golden/allof_merge.rs");
}
mod anyof_untagged {
    include!("golden/anyof_untagged.rs");
}
mod array_types {
    include!("golden/array_types.rs");
}
mod ext_x_rust_type {
    include!("golden/ext_x_rust_type.rs");
}
mod freeform_any {
    include!("golden/freeform_any.rs");
}
mod integer_formats {
    include!("golden/integer_formats.rs");
}
mod map_alias {
    include!("golden/map_alias.rs");
}
mod metadata_docs {
    include!("golden/metadata_docs.rs");
}
mod nullable {
    include!("golden/nullable.rs");
}
mod number_formats {
    include!("golden/number_formats.rs");
}
mod object_additional_properties {
    include!("golden/object_additional_properties.rs");
}
mod object_nested_inline {
    include!("golden/object_nested_inline.rs");
}
mod object_optional_required {
    include!("golden/object_optional_required.rs");
}
mod oneof_discriminator {
    include!("golden/oneof_discriminator.rs");
}
mod oneof_untagged {
    include!("golden/oneof_untagged.rs");
}
mod primitive_scalars {
    include!("golden/primitive_scalars.rs");
}
mod ref_local {
    include!("golden/ref_local.rs");
}
mod string_enum {
    include!("golden/string_enum.rs");
}
mod string_formats {
    include!("golden/string_formats.rs");
}

mod server_petstore {
    include!("golden/server_petstore.rs");
}

#[test]
fn untagged_enum_round_trips_through_serde() {
    let payment = oneof_discriminator::Payment::Card(oneof_discriminator::Card {
        number: "4111".to_owned(),
    });
    let json = serde_json::to_string(&payment).expect("serialize");
    let back: oneof_discriminator::Payment = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(payment, back);
}

#[test]
fn optional_field_is_skipped_when_none() {
    let profile = object_optional_required::Profile {
        id: "u1".to_owned(),
        nickname: None,
    };
    let json = serde_json::to_string(&profile).expect("serialize");
    assert_eq!(json, r#"{"id":"u1"}"#);
}

#[test]
fn generated_server_trait_implements_and_routes() {
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
            ListPetsResponse::Ok(Vec::new())
        }

        async fn create_pet(&self, body: NewPet) -> CreatePetResponse {
            CreatePetResponse::Created(Pet {
                id: 1,
                name: body.name,
                tag: body.tag,
            })
        }

        async fn get_pet(&self, id: i64) -> GetPetResponse {
            GetPetResponse::NotFound(Error {
                message: format!("no pet {id}"),
            })
        }

        async fn delete_pet(&self, _id: i64) -> DeletePetResponse {
            DeletePetResponse::NoContent
        }
    }

    // Building the router proves the native `async fn` trait, the response
    // enums' `IntoResponse`, and the handler wiring all type-check together.
    let _router: axum::Router = server_petstore::router(Service);
}
