//! Lowering OpenAPI schemas into the [`crate::ir`] representation.

use openapiv3::AdditionalProperties;
use openapiv3::Discriminator;
use openapiv3::IntegerFormat;
use openapiv3::ObjectType;
use openapiv3::ReferenceOr;
use openapiv3::Schema;
use openapiv3::SchemaData;
use openapiv3::SchemaKind;
use openapiv3::StringFormat;
use openapiv3::Type;
use openapiv3::VariantOrUnknownOrEmpty;

use crate::error::Error;
use crate::error::Result;
use crate::ir::Alias;
use crate::ir::Enum;
use crate::ir::EnumKind;
use crate::ir::Field;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RustType;
use crate::ir::StringVariant;
use crate::ir::Struct;
use crate::ir::UnionVariant;
use crate::loader::Spec;
use crate::loader::ref_target_name;
use crate::naming::Case;
use crate::naming::to_ident;

/// The `x-rust-type` extension: emit a verbatim Rust type expression.
const X_RUST_TYPE: &str = "x-rust-type";

/// Lower every component schema in `spec` into a module of Rust items.
pub fn generate_models(spec: &Spec) -> Result<Module> {
    let mut mapper = Mapper {
        spec,
        extra: Vec::new(),
    };
    let mut items = Vec::new();
    for (name, entry) in spec.schemas() {
        match entry {
            ReferenceOr::Item(schema) => {
                let item = mapper.named_to_item(name, schema)?;
                items.push(item);
            }
            ReferenceOr::Reference { reference } => {
                let target = ref_target_name(reference).ok_or_else(|| {
                    return Error::UnsupportedRef {
                        reference: reference.clone(),
                        reason: "top-level schema alias must reference another schema".to_owned(),
                    };
                })?;
                items.push(Item::Alias(Alias {
                    name: to_ident(name, Case::Pascal),
                    doc: None,
                    ty: RustType::Named(target.to_owned()),
                }));
            }
        }
    }
    items.append(&mut mapper.extra);
    return Ok(Module { items });
}

/// Lowers schemas into IR items, accumulating hoisted inline types in `extra`.
struct Mapper<'a> {
    spec: &'a Spec,
    extra: Vec<Item>,
}

impl Mapper<'_> {
    /// Lower a top-level named schema into a single item.
    fn named_to_item(&mut self, name: &str, schema: &Schema) -> Result<Item> {
        let data = &schema.schema_data;

        if let Some(verbatim) = extension_str(data, X_RUST_TYPE) {
            return Ok(Item::Alias(Alias {
                name: to_ident(name, Case::Pascal),
                doc: doc_of(data),
                ty: RustType::Verbatim(verbatim.to_owned()),
            }));
        }

        let item = match &schema.schema_kind {
            SchemaKind::Type(Type::String(st)) if !st.enumeration.is_empty() => {
                Item::Enum(self.string_enum(name, &st.enumeration, data))
            }
            SchemaKind::Type(Type::Object(obj)) => self.object_to_item(name, obj, data)?,
            SchemaKind::OneOf { one_of } | SchemaKind::AnyOf { any_of: one_of } => {
                Item::Enum(self.make_union(name, one_of, data)?)
            }
            SchemaKind::AllOf { all_of } => Item::Struct(self.merge_all_of(name, all_of, data)?),
            SchemaKind::Type(_) => {
                let ty = self.type_from_schema(name, schema)?;
                Item::Alias(Alias {
                    name: to_ident(name, Case::Pascal),
                    doc: doc_of(data),
                    ty,
                })
            }
            SchemaKind::Any(_) => Item::Alias(Alias {
                name: to_ident(name, Case::Pascal),
                doc: doc_of(data),
                ty: RustType::Value,
            }),
            SchemaKind::Not { .. } => {
                return Err(Error::UnsupportedSchema {
                    path: name.to_owned(),
                    reason: "`not` schemas are not supported".to_owned(),
                });
            }
        };
        return Ok(item);
    }

    /// Lower an object schema: a struct when it has properties, otherwise a map
    /// alias.
    fn object_to_item(&mut self, name: &str, obj: &ObjectType, data: &SchemaData) -> Result<Item> {
        if obj.properties.is_empty() {
            let element = self.additional_properties_type(name, obj)?;
            return Ok(Item::Alias(Alias {
                name: to_ident(name, Case::Pascal),
                doc: doc_of(data),
                ty: RustType::Map(Box::new(element)),
            }));
        }
        let strukt = self.object_to_struct(name, obj, data)?;
        return Ok(Item::Struct(strukt));
    }

    /// Build a struct from an object schema's properties.
    fn object_to_struct(&mut self, name: &str, obj: &ObjectType, data: &SchemaData) -> Result<Struct> {
        let mut fields = Vec::with_capacity(obj.properties.len());
        for (prop_name, prop) in &obj.properties {
            let required = obj.required.iter().any(|r| {
                return r == prop_name;
            });
            let field = self.field_from_prop(name, prop_name, prop, required)?;
            fields.push(field);
        }

        let additional_properties = match &obj.additional_properties {
            Some(AdditionalProperties::Schema(schema)) => {
                let ty = self.type_from_ref_schema(name, schema.as_ref())?;
                Some(ty)
            }
            Some(AdditionalProperties::Any(true)) => Some(RustType::Value),
            Some(AdditionalProperties::Any(false)) | None => None,
        };

        return Ok(Struct {
            name: to_ident(name, Case::Pascal),
            doc: doc_of(data),
            fields,
            additional_properties,
        });
    }

    /// Build a single struct field from a property schema.
    fn field_from_prop(
        &mut self,
        parent: &str,
        wire: &str,
        prop: &ReferenceOr<Box<Schema>>,
        required: bool,
    ) -> Result<Field> {
        let hint = format!("{parent}_{wire}");
        let mut ty = self.type_from_schema_ref(&hint, prop)?;

        let nullable = match prop {
            ReferenceOr::Item(schema) => schema.schema_data.nullable,
            ReferenceOr::Reference { .. } => false,
        };
        if !required || nullable {
            ty = ty.optional();
        }

        let doc = match prop {
            ReferenceOr::Item(schema) => doc_of(&schema.schema_data),
            ReferenceOr::Reference { .. } => None,
        };

        let ident = to_ident(wire, Case::Snake);
        let rename = crate::naming::rename_for(wire, &ident);
        return Ok(Field {
            name: ident,
            rename,
            doc,
            ty,
            required,
        });
    }

    /// Merge an `allOf` into a single flat struct, resolving `$ref` members to
    /// pull in their properties (matching oapi-codegen's behaviour).
    fn merge_all_of(&mut self, name: &str, members: &[ReferenceOr<Schema>], data: &SchemaData) -> Result<Struct> {
        let mut merged = MergedObject::default();
        self.absorb_members(name, members, &mut merged)?;

        let mut fields = Vec::with_capacity(merged.properties.len());
        for (wire, prop) in &merged.properties {
            let required = merged.required.iter().any(|r| {
                return r == wire;
            });
            let field = self.field_from_prop(name, wire, prop, required)?;
            fields.push(field);
        }

        return Ok(Struct {
            name: to_ident(name, Case::Pascal),
            doc: doc_of(data),
            fields,
            additional_properties: None,
        });
    }

    /// Recursively fold `allOf` members (objects, refs to objects, or nested
    /// `allOf`) into a single merged object.
    fn absorb_members(&mut self, name: &str, members: &[ReferenceOr<Schema>], merged: &mut MergedObject) -> Result<()> {
        for member in members {
            let schema = match member {
                ReferenceOr::Item(schema) => schema,
                ReferenceOr::Reference { reference } => self.spec.resolve(reference)?,
            };
            match &schema.schema_kind {
                SchemaKind::Type(Type::Object(obj)) => merged.absorb(obj),
                SchemaKind::AllOf { all_of } => self.absorb_members(name, all_of, merged)?,
                SchemaKind::Type(_)
                | SchemaKind::OneOf { .. }
                | SchemaKind::AnyOf { .. }
                | SchemaKind::Any(_)
                | SchemaKind::Not { .. } => {
                    return Err(Error::UnsupportedSchema {
                        path: name.to_owned(),
                        reason: "allOf members must be objects or refs to objects".to_owned(),
                    });
                }
            }
        }
        return Ok(());
    }
    fn make_union(&mut self, name: &str, members: &[ReferenceOr<Schema>], data: &SchemaData) -> Result<Enum> {
        let variants = match &data.discriminator {
            Some(disc) if !disc.mapping.is_empty() => self.union_variants_from_mapping(disc)?,
            Some(_) | None => self.union_variants_from_members(name, members)?,
        };
        return Ok(Enum {
            name: to_ident(name, Case::Pascal),
            doc: doc_of(data),
            kind: EnumKind::Union(variants),
        });
    }

    /// Variant list derived from a discriminator mapping (value -> $ref).
    fn union_variants_from_mapping(&self, disc: &Discriminator) -> Result<Vec<UnionVariant>> {
        let mut variants = Vec::with_capacity(disc.mapping.len());
        for (value, reference) in &disc.mapping {
            let target = ref_target_name(reference).ok_or_else(|| {
                return Error::UnsupportedRef {
                    reference: reference.clone(),
                    reason: "discriminator mapping must reference a schema".to_owned(),
                };
            })?;
            variants.push(UnionVariant {
                name: to_ident(value, Case::Pascal),
                ty: RustType::Named(target.to_owned()),
            });
        }
        return Ok(variants);
    }

    /// Variant list derived from the `oneOf`/`anyOf` member schemas directly.
    fn union_variants_from_members(
        &mut self,
        name: &str,
        members: &[ReferenceOr<Schema>],
    ) -> Result<Vec<UnionVariant>> {
        let mut variants = Vec::with_capacity(members.len());
        for (index, member) in members.iter().enumerate() {
            let variant = match member {
                ReferenceOr::Reference { reference } => {
                    let target = ref_target_name(reference).ok_or_else(|| {
                        return Error::UnsupportedRef {
                            reference: reference.clone(),
                            reason: "union member ref must reference a schema".to_owned(),
                        };
                    })?;
                    UnionVariant {
                        name: to_ident(target, Case::Pascal),
                        ty: RustType::Named(target.to_owned()),
                    }
                }
                ReferenceOr::Item(schema) => {
                    let hint = format!("{name}_variant_{index}");
                    let ty = self.type_from_schema(&hint, schema)?;
                    UnionVariant {
                        name: to_ident(&hint, Case::Pascal),
                        ty,
                    }
                }
            };
            variants.push(variant);
        }
        return Ok(variants);
    }

    /// Build a string enum from an OpenAPI string `enum`.
    fn string_enum(&self, name: &str, values: &[Option<String>], data: &SchemaData) -> Enum {
        let mut variants = Vec::new();
        for value in values.iter().flatten() {
            let ident = to_ident(value, Case::Pascal);
            let rename = crate::naming::rename_for(value, &ident);
            variants.push(StringVariant {
                name: ident,
                rename,
                doc: None,
            });
        }
        return Enum {
            name: to_ident(name, Case::Pascal),
            doc: doc_of(data),
            kind: EnumKind::Strings(variants),
        };
    }

    /// Resolve a property/items/additionalProperties schema reference to a type,
    /// hoisting inline named types into `self.extra` as needed.
    fn type_from_schema_ref(&mut self, hint: &str, schema: &ReferenceOr<Box<Schema>>) -> Result<RustType> {
        match schema {
            ReferenceOr::Reference { reference } => {
                let target = ref_target_name(reference).ok_or_else(|| {
                    return Error::UnsupportedRef {
                        reference: reference.clone(),
                        reason: "property ref must reference a schema".to_owned(),
                    };
                })?;
                return Ok(RustType::Named(target.to_owned()));
            }
            ReferenceOr::Item(schema) => {
                let ty = self.type_from_schema(hint, schema)?;
                return Ok(ty);
            }
        }
    }

    /// Resolve an `additionalProperties` schema (an unboxed `ReferenceOr`) to a
    /// type.
    fn type_from_ref_schema(&mut self, hint: &str, schema: &ReferenceOr<Schema>) -> Result<RustType> {
        match schema {
            ReferenceOr::Reference { reference } => {
                let target = ref_target_name(reference).ok_or_else(|| {
                    return Error::UnsupportedRef {
                        reference: reference.clone(),
                        reason: "additionalProperties ref must reference a schema".to_owned(),
                    };
                })?;
                return Ok(RustType::Named(target.to_owned()));
            }
            ReferenceOr::Item(schema) => {
                let ty = self.type_from_schema(hint, schema)?;
                return Ok(ty);
            }
        }
    }

    /// Map an inline schema to a Rust type, hoisting composite inline schemas
    /// into named items.
    fn type_from_schema(&mut self, hint: &str, schema: &Schema) -> Result<RustType> {
        let data = &schema.schema_data;
        if let Some(verbatim) = extension_str(data, X_RUST_TYPE) {
            return Ok(RustType::Verbatim(verbatim.to_owned()));
        }

        let ty = match &schema.schema_kind {
            SchemaKind::Type(Type::String(st)) if !st.enumeration.is_empty() => {
                let enom = self.string_enum(hint, &st.enumeration, data);
                self.extra.push(Item::Enum(enom));
                RustType::Named(hint.to_owned())
            }
            SchemaKind::Type(Type::String(st)) => string_format_type(&st.format),
            SchemaKind::Type(Type::Integer(it)) => integer_format_type(&it.format),
            SchemaKind::Type(Type::Number(_)) => RustType::F64,
            SchemaKind::Type(Type::Boolean(_)) => RustType::Bool,
            SchemaKind::Type(Type::Array(at)) => {
                let element = match &at.items {
                    Some(items) => {
                        let item_hint = format!("{hint}_item");
                        self.type_from_schema_ref(&item_hint, items)?
                    }
                    None => RustType::Value,
                };
                RustType::Vec(Box::new(element))
            }
            SchemaKind::Type(Type::Object(obj)) => self.inline_object_type(hint, obj, data)?,
            SchemaKind::OneOf { one_of } | SchemaKind::AnyOf { any_of: one_of } => {
                let enom = self.make_union(hint, one_of, data)?;
                self.extra.push(Item::Enum(enom));
                RustType::Named(hint.to_owned())
            }
            SchemaKind::AllOf { all_of } => {
                let strukt = self.merge_all_of(hint, all_of, data)?;
                self.extra.push(Item::Struct(strukt));
                RustType::Named(hint.to_owned())
            }
            SchemaKind::Any(_) => RustType::Value,
            SchemaKind::Not { .. } => {
                return Err(Error::UnsupportedSchema {
                    path: hint.to_owned(),
                    reason: "`not` schemas are not supported".to_owned(),
                });
            }
        };
        return Ok(ty);
    }

    /// Map an inline object: hoist a struct when it has properties, otherwise a
    /// map of its additionalProperties element type.
    fn inline_object_type(&mut self, hint: &str, obj: &ObjectType, data: &SchemaData) -> Result<RustType> {
        if obj.properties.is_empty() {
            let element = self.additional_properties_type(hint, obj)?;
            return Ok(RustType::Map(Box::new(element)));
        }
        let strukt = self.object_to_struct(hint, obj, data)?;
        self.extra.push(Item::Struct(strukt));
        return Ok(RustType::Named(hint.to_owned()));
    }

    /// Element type for an object used purely as a map (`additionalProperties`).
    fn additional_properties_type(&mut self, hint: &str, obj: &ObjectType) -> Result<RustType> {
        let element = match &obj.additional_properties {
            Some(AdditionalProperties::Schema(schema)) => self.type_from_ref_schema(hint, schema.as_ref())?,
            Some(AdditionalProperties::Any(_)) | None => RustType::Value,
        };
        return Ok(element);
    }
}

/// Accumulates merged properties of an `allOf`, preserving first-seen order.
#[derive(Default)]
struct MergedObject {
    properties: indexmap::IndexMap<String, ReferenceOr<Box<Schema>>>,
    required: Vec<String>,
}

impl MergedObject {
    /// Fold one object schema's properties and required list into the merge.
    fn absorb(&mut self, obj: &ObjectType) {
        for (name, prop) in &obj.properties {
            self.properties.insert(name.clone(), prop.clone());
        }
        for req in &obj.required {
            if !self.required.contains(req) {
                self.required.push(req.clone());
            }
        }
    }
}

/// Map a string `format` to a Rust type.
fn string_format_type(format: &VariantOrUnknownOrEmpty<StringFormat>) -> RustType {
    let ty = match format {
        VariantOrUnknownOrEmpty::Item(StringFormat::Date) => RustType::Date,
        VariantOrUnknownOrEmpty::Item(StringFormat::DateTime) => RustType::DateTime,
        VariantOrUnknownOrEmpty::Item(StringFormat::Byte) => RustType::Bytes,
        VariantOrUnknownOrEmpty::Item(StringFormat::Binary) => RustType::Bytes,
        VariantOrUnknownOrEmpty::Item(StringFormat::Password) => RustType::String,
        VariantOrUnknownOrEmpty::Unknown(name) if name == "uuid" => RustType::Uuid,
        VariantOrUnknownOrEmpty::Unknown(_) => RustType::String,
        VariantOrUnknownOrEmpty::Empty => RustType::String,
    };
    return ty;
}

/// Map an integer `format` to a Rust type.
fn integer_format_type(format: &VariantOrUnknownOrEmpty<IntegerFormat>) -> RustType {
    let ty = match format {
        VariantOrUnknownOrEmpty::Item(IntegerFormat::Int32) => RustType::I32,
        VariantOrUnknownOrEmpty::Item(IntegerFormat::Int64) => RustType::I64,
        VariantOrUnknownOrEmpty::Unknown(_) | VariantOrUnknownOrEmpty::Empty => RustType::I64,
    };
    return ty;
}

/// Extract a string-valued extension (e.g. `x-rust-type`) from schema data.
fn extension_str<'a>(data: &'a SchemaData, key: &str) -> Option<&'a str> {
    let value = data.extensions.get(key)?;
    return value.as_str();
}

/// Trim and normalise a schema `description` into a doc comment.
fn doc_of(data: &SchemaData) -> Option<String> {
    let text = data.description.as_ref()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    return Some(trimmed.to_owned());
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Parse an inline OpenAPI document and emit the generated Rust source.
    fn emit_yaml(yaml: &str) -> String {
        let doc: openapiv3::OpenAPI = serde_yaml::from_str(yaml).expect("parse spec");
        let spec = Spec::from_parts(doc, PathBuf::from("inline.yaml"));
        let module = generate_models(&spec).expect("map schemas");
        return crate::emit::emit_module(&module).expect("emit module");
    }

    const PREAMBLE: &str = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  schemas:\n";

    #[test]
    fn maps_string_and_integer_formats() {
        let yaml = format!(
            "{PREAMBLE}    Thing:\n      type: object\n      required: [day, at, id, blob, big]\n      properties:\n        day:\n          type: string\n          format: date\n        at:\n          type: string\n          format: date-time\n        id:\n          type: string\n          format: uuid\n        blob:\n          type: string\n          format: byte\n        big:\n          type: integer\n          format: int64\n"
        );
        let out = emit_yaml(&yaml);
        assert!(out.contains("pub day: chrono::NaiveDate"), "{out}");
        assert!(out.contains("pub at: chrono::DateTime<chrono::Utc>"), "{out}");
        assert!(out.contains("pub id: uuid::Uuid"), "{out}");
        assert!(out.contains("pub blob: Vec<u8>"), "{out}");
        assert!(out.contains("pub big: i64"), "{out}");
    }

    #[test]
    fn object_with_only_additional_properties_becomes_map_alias() {
        let yaml =
            format!("{PREAMBLE}    Dict:\n      type: object\n      additionalProperties:\n        type: string\n");
        let out = emit_yaml(&yaml);
        assert!(
            out.contains("pub type Dict = std::collections::HashMap<String, String>;"),
            "{out}"
        );
    }

    #[test]
    fn inline_nested_object_is_hoisted() {
        let yaml = format!(
            "{PREAMBLE}    Outer:\n      type: object\n      required: [inner]\n      properties:\n        inner:\n          type: object\n          required: [x]\n          properties:\n            x:\n              type: string\n"
        );
        let out = emit_yaml(&yaml);
        assert!(out.contains("pub struct Outer"), "{out}");
        assert!(out.contains("pub inner: OuterInner"), "{out}");
        assert!(out.contains("pub struct OuterInner"), "{out}");
        assert!(out.contains("pub x: String"), "{out}");
    }

    #[test]
    fn x_rust_type_emits_verbatim_type() {
        let yaml = format!(
            "{PREAMBLE}    Holder:\n      type: object\n      required: [v]\n      properties:\n        v:\n          type: string\n          x-rust-type: my_crate::Custom\n"
        );
        let out = emit_yaml(&yaml);
        assert!(out.contains("pub v: my_crate::Custom"), "{out}");
    }

    #[test]
    fn optional_field_is_wrapped_and_skipped() {
        let yaml = format!(
            "{PREAMBLE}    Maybe:\n      type: object\n      properties:\n        note:\n          type: string\n"
        );
        let out = emit_yaml(&yaml);
        assert!(out.contains("skip_serializing_if = \"Option::is_none\""), "{out}");
        assert!(out.contains("pub note: Option<String>"), "{out}");
    }
}
