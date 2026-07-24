//! Deriving the Rust names of the artifacts generated for one OpenAPI operation.
//!
//! Centralised so the server emitter and (later) the client emitter derive the
//! same names from the same operation identifier, and cannot drift apart.

use crate::naming::Case;
use crate::naming::RustIdent;
use crate::naming::to_ident;

pub fn operation_method_name(raw: &str) -> RustIdent {
    return to_ident(raw, Case::Snake);
}

/// The generated response-enum name for an operation, `<Op><Suffix>` (the
/// suffix defaults to `Response`; override it via `response-type-suffix` to
/// resolve a clash with a component schema of the same name).
pub fn response_enum_name(op: &RustIdent, suffix: &str) -> RustIdent {
    return to_ident(&format!("{}_{}", op.logical(), suffix), Case::Pascal);
}

pub fn query_struct_name(op: &RustIdent) -> RustIdent {
    return to_ident(&format!("{}_query", op.logical()), Case::Pascal);
}

pub fn headers_struct_name(op: &RustIdent) -> RustIdent {
    return to_ident(&format!("{}_headers", op.logical()), Case::Pascal);
}

/// The generated cookie-struct name for an operation (`<Op>Cookies`).
pub fn cookies_struct_name(op: &RustIdent) -> RustIdent {
    return to_ident(&format!("{}_cookies", op.logical()), Case::Pascal);
}

/// The generated multipart-extractor struct name for an operation
/// (`<Op>Multipart`).
pub fn multipart_struct_name(op: &RustIdent) -> RustIdent {
    return to_ident(&format!("{}_multipart", op.logical()), Case::Pascal);
}

/// The generated dispatch-enum name for an operation whose request body offers
/// several content types (`<Op>RequestBody`).
pub fn request_body_enum_name(op: &RustIdent) -> RustIdent {
    return to_ident(&format!("{}_request_body", op.logical()), Case::Pascal);
}

/// The generated body-enum name for a response variant that offers several
/// content types (`<Response><Variant>Body`).
pub fn response_body_enum_name(response_enum: &RustIdent, variant: &RustIdent) -> RustIdent {
    return to_ident(
        &format!("{}_{}_body", response_enum.logical(), variant.logical()),
        Case::Pascal,
    );
}

pub fn axum_handler_name(op: &RustIdent) -> RustIdent {
    return to_ident(&format!("{}_handler", op.logical()), Case::Snake);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(name: &str) -> RustIdent {
        return to_ident(name, Case::Snake);
    }

    #[test]
    fn derives_operation_artifact_names() {
        let list_pets = op("list_pets");
        assert_eq!(operation_method_name("listPets").logical(), "list_pets");
        assert_eq!(response_enum_name(&list_pets, "response").logical(), "ListPetsResponse");
        assert_eq!(query_struct_name(&list_pets).logical(), "ListPetsQuery");
        assert_eq!(headers_struct_name(&list_pets).logical(), "ListPetsHeaders");
        assert_eq!(cookies_struct_name(&list_pets).logical(), "ListPetsCookies");
        assert_eq!(multipart_struct_name(&list_pets).logical(), "ListPetsMultipart");
        assert_eq!(request_body_enum_name(&list_pets).logical(), "ListPetsRequestBody");
        assert_eq!(response_enum_name(&list_pets, "Resp").logical(), "ListPetsResp");
        let response = response_enum_name(&list_pets, "response");
        let ok = to_ident("ok", Case::Pascal);
        assert_eq!(
            response_body_enum_name(&response, &ok).logical(),
            "ListPetsResponseOkBody"
        );
        assert_eq!(axum_handler_name(&list_pets).logical(), "list_pets_handler");
    }
}
