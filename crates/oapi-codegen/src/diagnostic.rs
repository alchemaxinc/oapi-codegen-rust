use anstream::eprintln;
use owo_colors::OwoColorize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Warning {
    pub(crate) path: String,
    pub(crate) message: String,
}

impl Warning {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        return Self {
            path: path.into(),
            message: message.into(),
        };
    }
}

pub(crate) fn report_warnings(document: &str, warnings: &[Warning]) {
    for warning in warnings {
        eprintln!(
            "{} {document}: {}: {}",
            "warning:".yellow().bold(),
            warning.path,
            warning.message
        );
    }
}

pub(crate) fn pointer(parent: &str, key: &str) -> String {
    return format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_escapes_literal_keys() {
        for (parent, key, expected) in [
            ("", "generate", "/generate"),
            ("/generate", "modles", "/generate/modles"),
            ("", "a~/b", "/a~0~1b"),
            ("/a~0~1b", "~1/", "/a~0~1b/~01~1"),
            ("", "", "/"),
        ] {
            assert_eq!(pointer(parent, key), expected);
        }
    }
}
