use std::collections::HashMap;

use openapiv3::ReferenceOr;
use openapiv3::Schema;
use serde_json::Value;

use crate::error::Error;
use crate::error::Result;
use crate::ir::UnionValidation;
use crate::ir::UnionValidationNode;
use crate::loader::Spec;

pub(super) fn validation(spec: &Spec, member: &ReferenceOr<Schema>) -> Result<UnionValidation> {
    let value = serde_json::to_value(member).map_err(|error| {
        return Error::UnsupportedSchema {
            path: "union alternative".to_owned(),
            reason: error.to_string(),
        };
    })?;
    let mut graph = UnionValidation::default();
    add_node(spec, &value, &mut graph, &mut HashMap::new())?;
    return Ok(graph);
}

fn add_node(
    spec: &Spec,
    value: &Value,
    graph: &mut UnionValidation,
    references: &mut HashMap<String, usize>,
) -> Result<usize> {
    if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        if let Some(index) = references.get(reference) {
            return Ok(*index);
        }
        let index = graph.nodes.len();
        references.insert(reference.to_owned(), index);
        let schema = serde_json::to_value(spec.resolve(reference)?).map_err(|error| {
            return Error::UnsupportedSchema {
                path: reference.to_owned(),
                reason: error.to_string(),
            };
        })?;
        return add_node(spec, &schema, graph, references);
    }
    let index = graph.nodes.len();
    let mut keywords = value.as_object().cloned().unwrap_or_default();
    if let Some(pattern) = keywords.get("pattern").and_then(Value::as_str) {
        regex::Regex::new(pattern).map_err(|error| {
            return Error::UnsupportedSchema {
                path: "union alternative.pattern".to_owned(),
                reason: error.to_string(),
            };
        })?;
    }
    keywords.retain(|key, value| {
        return match key.as_str() {
            "type" | "nullable" | "enum" | "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum"
            | "multipleOf" | "pattern" | "format" | "minLength" | "maxLength" | "minItems" | "maxItems"
            | "uniqueItems" | "minProperties" | "maxProperties" | "required" | "readOnly" | "writeOnly" => true,
            "additionalProperties" => value.is_boolean(),
            _ => false,
        };
    });
    graph.nodes.push(UnionValidationNode {
        keywords: keywords.clone(),
        properties: Vec::new(),
        items: None,
        additional: None,
        children: Vec::new(),
    });
    let mut properties = Vec::new();
    if let Some(object) = value.get("properties").and_then(Value::as_object) {
        for (name, child) in object {
            properties.push((name.clone(), add_node(spec, child, graph, references)?));
        }
    }
    let items = value
        .get("items")
        .map(|child| return add_node(spec, child, graph, references))
        .transpose()?;
    let additional = value
        .get("additionalProperties")
        .filter(|child| return child.is_object())
        .map(|child| return add_node(spec, child, graph, references))
        .transpose()?;
    let mut children = Vec::new();
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(members) = value.get(key).and_then(Value::as_array) {
            keywords.insert(key.to_owned(), Value::Bool(true));
            for child in members {
                children.push(add_node(spec, child, graph, references)?);
            }
        }
    }
    let node = graph.nodes.get_mut(index).ok_or_else(|| {
        return Error::UnsupportedSchema {
            path: "union alternative".to_owned(),
            reason: "the validation graph has no root node".to_owned(),
        };
    })?;
    *node = UnionValidationNode {
        keywords,
        properties,
        items,
        additional,
        children,
    };
    return Ok(index);
}
