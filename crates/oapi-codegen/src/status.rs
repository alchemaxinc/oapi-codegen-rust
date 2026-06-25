//! Mapping HTTP status codes to `axum::http::StatusCode` associated constants.
//!
//! The server generator names each response-enum variant after the canonical
//! reason phrase (the `StatusCode` constant `PascalCase`d) and emits the matching
//! `axum::http::StatusCode::CONSTANT` when building the response.

/// Standard status codes paired with their `axum::http::StatusCode` constant.
static STATUS_CONSTANTS: &[(u16, &str)] = &[
    (100, "CONTINUE"),
    (101, "SWITCHING_PROTOCOLS"),
    (102, "PROCESSING"),
    (200, "OK"),
    (201, "CREATED"),
    (202, "ACCEPTED"),
    (203, "NON_AUTHORITATIVE_INFORMATION"),
    (204, "NO_CONTENT"),
    (205, "RESET_CONTENT"),
    (206, "PARTIAL_CONTENT"),
    (207, "MULTI_STATUS"),
    (208, "ALREADY_REPORTED"),
    (226, "IM_USED"),
    (300, "MULTIPLE_CHOICES"),
    (301, "MOVED_PERMANENTLY"),
    (302, "FOUND"),
    (303, "SEE_OTHER"),
    (304, "NOT_MODIFIED"),
    (305, "USE_PROXY"),
    (307, "TEMPORARY_REDIRECT"),
    (308, "PERMANENT_REDIRECT"),
    (400, "BAD_REQUEST"),
    (401, "UNAUTHORIZED"),
    (402, "PAYMENT_REQUIRED"),
    (403, "FORBIDDEN"),
    (404, "NOT_FOUND"),
    (405, "METHOD_NOT_ALLOWED"),
    (406, "NOT_ACCEPTABLE"),
    (407, "PROXY_AUTHENTICATION_REQUIRED"),
    (408, "REQUEST_TIMEOUT"),
    (409, "CONFLICT"),
    (410, "GONE"),
    (411, "LENGTH_REQUIRED"),
    (412, "PRECONDITION_FAILED"),
    (413, "PAYLOAD_TOO_LARGE"),
    (414, "URI_TOO_LONG"),
    (415, "UNSUPPORTED_MEDIA_TYPE"),
    (416, "RANGE_NOT_SATISFIABLE"),
    (417, "EXPECTATION_FAILED"),
    (418, "IM_A_TEAPOT"),
    (421, "MISDIRECTED_REQUEST"),
    (422, "UNPROCESSABLE_ENTITY"),
    (423, "LOCKED"),
    (424, "FAILED_DEPENDENCY"),
    (425, "TOO_EARLY"),
    (426, "UPGRADE_REQUIRED"),
    (428, "PRECONDITION_REQUIRED"),
    (429, "TOO_MANY_REQUESTS"),
    (431, "REQUEST_HEADER_FIELDS_TOO_LARGE"),
    (451, "UNAVAILABLE_FOR_LEGAL_REASONS"),
    (500, "INTERNAL_SERVER_ERROR"),
    (501, "NOT_IMPLEMENTED"),
    (502, "BAD_GATEWAY"),
    (503, "SERVICE_UNAVAILABLE"),
    (504, "GATEWAY_TIMEOUT"),
    (505, "HTTP_VERSION_NOT_SUPPORTED"),
    (506, "VARIANT_ALSO_NEGOTIATES"),
    (507, "INSUFFICIENT_STORAGE"),
    (508, "LOOP_DETECTED"),
    (510, "NOT_EXTENDED"),
    (511, "NETWORK_AUTHENTICATION_REQUIRED"),
];

/// The `axum::http::StatusCode` constant name for a numeric status code, if it
/// is a recognised standard code.
pub fn const_name(code: u16) -> Option<&'static str> {
    let name = STATUS_CONSTANTS.iter().find_map(|(candidate, name)| {
        if *candidate == code {
            return Some(*name);
        }
        return None;
    });
    return name;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::Case;
    use crate::naming::to_ident;

    #[test]
    fn maps_common_codes() {
        assert_eq!(const_name(200), Some("OK"));
        assert_eq!(const_name(404), Some("NOT_FOUND"));
        assert_eq!(const_name(500), Some("INTERNAL_SERVER_ERROR"));
        assert_eq!(const_name(599), None);
    }

    #[test]
    fn variant_names_are_pascal_case() {
        let variant = |code| return to_ident(const_name(code).unwrap(), Case::Pascal);
        assert_eq!(variant(200).logical(), "Ok");
        assert_eq!(variant(204).logical(), "NoContent");
        assert_eq!(variant(500).logical(), "InternalServerError");
    }
}
