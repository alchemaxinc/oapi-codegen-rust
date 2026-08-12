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
use crate::ir::Deprecation;
use crate::ir::Enum;
use crate::ir::EnumKind;
use crate::ir::Field;
use crate::ir::ForeignDerives;
use crate::ir::IntegerVariant;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RustType;
use crate::ir::StringVariant;
use crate::ir::Struct;
use crate::ir::UnionVariant;
use crate::loader::Spec;
use crate::loader::ref_target_name;
use crate::loader::schema_ref_reason;
use crate::lower::default::lower_default;
use crate::naming::Case;
use crate::naming::X_RUST_NAME;
use crate::naming::to_ident;

/// The `x-rust-type` extension: emit a verbatim Rust type expression.
pub(crate) const X_RUST_TYPE: &str = "x-rust-type";
/// The `x-rust-derive` extension: which of `Debug`, `Clone`, `PartialEq` an
/// `x-rust-type` target implements.
const X_RUST_DERIVE: &str = "x-rust-derive";
/// The `x-rust-serde-skip` extension: drop a field via `#[serde(skip)]`.
const X_RUST_SERDE_SKIP: &str = "x-rust-serde-skip";
/// The `x-omitempty` extension: force `skip_serializing_if` on/off for a field.
const X_OMITEMPTY: &str = "x-omitempty";
/// The `x-order` extension: explicitly order struct fields (1-indexed).
const X_ORDER: &str = "x-order";
/// The `x-deprecated-reason` extension: the note for a `#[deprecated]` item.
const X_DEPRECATED_REASON: &str = "x-deprecated-reason";
/// The `x-enum-varnames` extension: override generated enum variant identifiers.
const X_ENUM_VARNAMES: &str = "x-enum-varnames";
/// The `x-enumNames` extension: alias of [`X_ENUM_VARNAMES`].
const X_ENUM_NAMES: &str = "x-enumNames";

/// Cap on inline schema nesting the lowering pass will descend before erroring.
/// Guards against stack exhaustion on pathological or hostile specs. Well above any
/// realistic hand-written or generated spec, and independent of whatever
/// recursion limit the YAML/JSON parser happens to enforce.
const MAX_SCHEMA_DEPTH: usize = 100;

/// Lower every component schema in `spec` into a module of Rust items.
///
/// `names` holds the final Rust type name of every schema, from
/// [`crate::lower::rename::type_renames`]. The caller resolves the names first,
/// because it also applies them to the service and decides when to report an
/// unresolved collision. A module built from unchecked `names` can hold two items
/// with one name, so the caller must check `names` before the emit pass.
pub fn generate_models(spec: &Spec, names: &crate::lower::rename::TypeNames) -> Result<Module> {
    let renames = names.renames();
    let mut mapper = Mapper {
        spec,
        renames,
        extra: Vec::new(),
        depth: 0,
    };
    let mut items = Vec::new();
    for (name, entry) in spec.schemas() {
        match entry {
            ReferenceOr::Item(schema) => {
                let item = mapper.named_to_item(name, schema)?;
                items.push(item);
            }
            ReferenceOr::Reference { reference } => {
                let target = mapper.schema_ref_target(reference, "a top-level schema alias")?;
                items.push(Item::Alias(Alias {
                    name: mapper.type_name_ident(name),
                    doc: None,
                    deprecated: None,
                    ty: RustType::Named(target),
                }));
            }
        }
    }
    items.append(&mut mapper.extra);
    let mut module = Module { items };
    crate::lower::rename::rewrite_module(&mut module, renames);
    return Ok(module);
}

/// Lowers schemas into IR items, accumulating hoisted inline types in `extra`.
struct Mapper<'a> {
    spec: &'a Spec,
    /// `x-rust-name` overrides keyed by original schema name.
    renames: &'a std::collections::HashMap<String, String>,
    extra: Vec<Item>,
    /// Current inline-nesting depth, bounded by [`MAX_SCHEMA_DEPTH`].
    depth: usize,
}

impl Mapper<'_> {
    /// The name a schema `$ref` at `site` points to.
    ///
    /// Two faults end generation here. The ref can have a form no schema site
    /// accepts, which [`schema_ref_reason`] describes. The ref can also name a
    /// schema the document does not declare. Without the second check the name
    /// reaches the output, and the generated file does not compile.
    fn schema_ref_target(&self, reference: &str, site: &str) -> Result<String> {
        let target = ref_target_name(reference).ok_or_else(|| {
            return Error::UnsupportedRef {
                reference: reference.to_owned(),
                reason: schema_ref_reason(reference, site),
            };
        })?;
        if !self.spec.schemas().contains_key(target) {
            return Err(Error::UnresolvedRef(reference.to_owned()));
        }
        return Ok(target.to_owned());
    }

    /// The identifier for a top-level type, honouring an `x-rust-name` override.
    fn type_name_ident(&self, name: &str) -> crate::naming::RustIdent {
        let effective = self.renames.get(name).map(String::as_str).unwrap_or(name);
        return to_ident(effective, Case::Pascal);
    }

    /// Lower a top-level named schema into a single item.
    fn named_to_item(&mut self, name: &str, schema: &Schema) -> Result<Item> {
        let data = &schema.schema_data;

        if let Some(verbatim) = extension_str(data, X_RUST_TYPE, name)? {
            return Ok(Item::Alias(Alias {
                name: self.type_name_ident(name),
                doc: doc_of(data),
                deprecated: deprecation_of(data, name)?,
                ty: verbatim_type(data, verbatim, name)?,
            }));
        }

        let item = match &schema.schema_kind {
            SchemaKind::Type(Type::String(st)) if !st.enumeration.is_empty() => {
                Item::Enum(self.string_enum(name, &st.enumeration, data)?)
            }
            SchemaKind::Type(Type::Integer(it)) if !it.enumeration.is_empty() => {
                let repr = integer_format_type(&it.format);
                Item::Enum(self.integer_enum(name, &it.enumeration, &repr, data)?)
            }
            SchemaKind::Type(Type::Object(obj)) => self.object_to_item(name, obj, data)?,
            SchemaKind::OneOf { one_of } | SchemaKind::AnyOf { any_of: one_of } => {
                Item::Enum(self.make_union(name, one_of, data)?)
            }
            SchemaKind::AllOf { all_of } => match self.single_ref_all_of(all_of)? {
                Some(target) => Item::Alias(Alias {
                    name: self.type_name_ident(name),
                    doc: doc_of(data),
                    deprecated: deprecation_of(data, name)?,
                    ty: RustType::Named(target),
                }),
                None => Item::Struct(self.merge_all_of(name, all_of, data)?),
            },
            SchemaKind::Type(_) => {
                let ty = self.type_from_schema(name, schema)?;
                Item::Alias(Alias {
                    name: self.type_name_ident(name),
                    doc: doc_of(data),
                    deprecated: deprecation_of(data, name)?,
                    ty,
                })
            }
            SchemaKind::Any(_) => Item::Alias(Alias {
                name: self.type_name_ident(name),
                doc: doc_of(data),
                deprecated: deprecation_of(data, name)?,
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
                name: self.type_name_ident(name),
                doc: doc_of(data),
                deprecated: deprecation_of(data, name)?,
                ty: RustType::Map(Box::new(element)),
            }));
        }
        let strukt = self.object_to_struct(name, obj, data)?;
        return Ok(Item::Struct(strukt));
    }

    /// Build a struct from an object schema's properties.
    fn object_to_struct(&mut self, name: &str, obj: &ObjectType, data: &SchemaData) -> Result<Struct> {
        let mut ordered = Vec::with_capacity(obj.properties.len());
        for (prop_name, prop) in &obj.properties {
            let required = obj.required.iter().any(|r| {
                return r == prop_name;
            });
            let order = prop_order(prop, &format!("{name}.{prop_name}"))?;
            let field = self.field_from_prop(name, prop_name, prop, required)?;
            ordered.push((order, field));
        }
        let fields = sort_by_order(ordered);

        let additional_properties = match &obj.additional_properties {
            Some(AdditionalProperties::Schema(schema)) => {
                let ty = self.type_from_ref_schema(name, schema.as_ref())?;
                Some(ty)
            }
            Some(AdditionalProperties::Any(true)) => Some(RustType::Value),
            Some(AdditionalProperties::Any(false)) | None => None,
        };
        // Only an explicit `false` denies unknown keys. An absent key permits
        // them, which is serde's behaviour with no attribute.
        let deny_unknown_fields = matches!(&obj.additional_properties, Some(AdditionalProperties::Any(false)));

        return Ok(Struct {
            name: self.type_name_ident(name),
            doc: doc_of(data),
            deprecated: deprecation_of(data, name)?,
            fields,
            additional_properties,
            deny_unknown_fields,
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
        let at = format!("{parent}.{wire}");
        let mut ty = self.type_from_schema_ref(&hint, prop)?;

        let data = match prop {
            ReferenceOr::Item(schema) => Some(&schema.schema_data),
            ReferenceOr::Reference { .. } => None,
        };

        // A required property is always present, so its `default` never
        // applies. Use it, and a payload that omits a required property becomes
        // valid.
        let declared = data
            .filter(|_| return !required)
            .and_then(|data| return data.default.as_ref());

        let nullable = data.map(|data| return data.nullable).unwrap_or(false);
        // With a default, an absent property ends up the same as a present one,
        // so `Option` would only ever hold `Some`. `nullable` is the exception,
        // because there `null` is a value the property carries.
        if (!required && declared.is_none()) || nullable {
            ty = ty.optional();
        }

        let default = match declared {
            Some(json) => {
                let variants_of = |name: &str| {
                    let ident = self.type_name_ident(name);
                    return self.extra.iter().find_map(|item| {
                        return match item {
                            Item::Enum(enom) if enom.name == ident => match &enom.kind {
                                EnumKind::Strings(variants) => Some(variants.clone()),
                                EnumKind::Union(_) | EnumKind::Integers { .. } => None,
                            },
                            _ => None,
                        };
                    });
                };
                Some(lower_default(json, &ty, &variants_of, parent, wire)?)
            }
            None => None,
        };

        let doc = data.and_then(doc_of);
        let deprecated = match data {
            Some(data) => deprecation_of(data, &at)?,
            None => None,
        };
        let serde_skip = match data {
            Some(data) => extension_bool(data, X_RUST_SERDE_SKIP, &at)?.unwrap_or(false),
            None => false,
        };
        let omit_empty = match data {
            Some(data) => extension_bool(data, X_OMITEMPTY, &at)?,
            None => None,
        };

        let rust_name = match data {
            Some(data) => extension_str(data, X_RUST_NAME, &at)?,
            None => None,
        };
        let ident = match rust_name {
            Some(custom) => to_ident(custom, Case::Snake),
            None => to_ident(wire, Case::Snake),
        };
        let rename = crate::naming::rename_for(wire, &ident);
        let constraints = match prop {
            ReferenceOr::Item(schema) => crate::lower::constraints::constraints_of(schema),
            // The alias a `$ref` makes carries no serde attribute, so the field
            // takes the checks the target declares.
            ReferenceOr::Reference { reference } => self
                .spec
                .resolve(reference)
                .ok()
                .and_then(crate::lower::constraints::constraints_through_ref),
        };
        let field = Field {
            name: ident,
            rename,
            doc,
            deprecated,
            ty,
            required,
            omit_empty,
            serde_skip,
            default,
            constraints,
        };
        crate::lower::constraints::check_constraints(&field)?;
        return Ok(field);
    }

    /// Return the referenced schema name when `members` is a single `$ref`
    /// member, for collapsing a one-element `allOf` at the top level into a type
    /// alias. Only `$ref` members qualify: a single inline member is left to the
    /// struct-merge path so a schema never aliases itself.
    fn single_ref_all_of(&self, members: &[ReferenceOr<Schema>]) -> Result<Option<String>> {
        let [ReferenceOr::Reference { reference }] = members else {
            return Ok(None);
        };
        let target = self.schema_ref_target(reference, "an allOf member")?;
        return Ok(Some(target));
    }

    /// Collapse a single-member `allOf` to the type of its sole member.
    ///
    /// A one-element `allOf` carries no composition — it exists only to attach
    /// sibling keywords (`nullable`, `description`) to a `$ref`, which is the
    /// canonical OpenAPI 3.0 way to annotate or make a reference nullable. In
    /// that case the wrapper must resolve to the referenced type itself (reusing
    /// the shared named schema, and working for enum/union targets too) rather
    /// than synthesizing a duplicate struct. Multi-member `allOf` is genuine
    /// composition and returns `None` so the caller merges it as before.
    fn collapse_single_all_of(&mut self, hint: &str, members: &[ReferenceOr<Schema>]) -> Result<Option<RustType>> {
        let [only] = members else {
            return Ok(None);
        };
        let ty = match only {
            ReferenceOr::Reference { reference } => {
                let target = self.schema_ref_target(reference, "an allOf member")?;
                RustType::Named(target)
            }
            ReferenceOr::Item(schema) => self.type_from_schema(hint, schema)?,
        };
        return Ok(Some(ty));
    }

    /// Merge an `allOf` into a single flat struct, resolving `$ref` members to
    /// pull in their properties (matching oapi-codegen's behaviour).
    fn merge_all_of(&mut self, name: &str, members: &[ReferenceOr<Schema>], data: &SchemaData) -> Result<Struct> {
        let mut merged = MergedObject::default();
        self.absorb_members(name, members, &mut merged)?;

        let mut ordered = Vec::with_capacity(merged.properties.len());
        for (wire, prop) in &merged.properties {
            let required = merged.required.iter().any(|r| {
                return r == wire;
            });
            let order = prop_order(prop, &format!("{name}.{wire}"))?;
            let field = self.field_from_prop(name, wire, prop, required)?;
            ordered.push((order, field));
        }
        let fields = sort_by_order(ordered);

        return Ok(Struct {
            name: self.type_name_ident(name),
            doc: doc_of(data),
            deprecated: deprecation_of(data, name)?,
            fields,
            additional_properties: None,
            // A merge does not read `additionalProperties` from any member. In
            // JSON Schema each `allOf` member validates the whole object, so a
            // member with `additionalProperties: false` rejects every property
            // that a sibling member declares. A merge that honoured it would
            // deny the fields it just merged in. The merge drops the key, as it
            // already drops a member's `additionalProperties` schema.
            deny_unknown_fields: false,
        });
    }

    /// Recursively fold `allOf` members (objects, refs to objects, or nested
    /// `allOf`) into a single merged object. Shares the [`MAX_SCHEMA_DEPTH`]
    /// counter with [`Self::type_from_schema`] so nested `allOf` cannot exhaust
    /// the stack independently of inline-type nesting.
    fn absorb_members(&mut self, name: &str, members: &[ReferenceOr<Schema>], merged: &mut MergedObject) -> Result<()> {
        if self.depth >= MAX_SCHEMA_DEPTH {
            return Err(Error::SchemaDepthExceeded {
                path: name.to_owned(),
                limit: MAX_SCHEMA_DEPTH,
            });
        }
        self.depth += 1;
        let result = self.absorb_members_inner(name, members, merged);
        self.depth -= 1;
        return result;
    }

    fn absorb_members_inner(
        &mut self,
        name: &str,
        members: &[ReferenceOr<Schema>],
        merged: &mut MergedObject,
    ) -> Result<()> {
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
        check_variant_types(name, &variants)?;
        return Ok(Enum {
            name: self.type_name_ident(name),
            doc: doc_of(data),
            deprecated: deprecation_of(data, name)?,
            kind: EnumKind::Union(variants),
        });
    }

    /// Variant list derived from a discriminator mapping (value -> $ref).
    fn union_variants_from_mapping(&self, disc: &Discriminator) -> Result<Vec<UnionVariant>> {
        let mut variants = Vec::with_capacity(disc.mapping.len());
        let mut seen = std::collections::HashSet::new();
        for (value, reference) in &disc.mapping {
            let target = self.schema_ref_target(reference, "a discriminator mapping")?;
            variants.push(UnionVariant {
                name: crate::naming::deconflict_ident(to_ident(value, Case::Pascal), &mut seen),
                ty: RustType::Named(target),
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
        let mut seen = std::collections::HashSet::new();
        let mut diagnostics = crate::lower::validate::Diagnostics::new();
        for (index, member) in members.iter().enumerate() {
            let variant = match member {
                ReferenceOr::Reference { reference } => {
                    let target = self.schema_ref_target(reference, "a union member")?;
                    // Name the variant from the *resolved* type name, so an
                    // `x-rust-name` override or a configured collision suffix
                    // reaches the variant too. The raw target name would give a
                    // variant that contradicts its own payload type.
                    UnionVariant {
                        name: crate::naming::deconflict_ident(self.type_name_ident(&target), &mut seen),
                        ty: RustType::Named(target),
                    }
                }
                ReferenceOr::Item(schema) => {
                    let Some(seed) = inline_variant_seed(schema, &format!("{name}, member {index}"))? else {
                        diagnostics.push(Error::UnsupportedSchema {
                            path: name.to_owned(),
                            reason: format!("member {index} of the union gives the variant no name"),
                        });
                        continue;
                    };
                    // The seed names the variant and any type the member hoists,
                    // so the two agree.
                    let ty = self.type_from_schema(&seed, schema)?;
                    UnionVariant {
                        name: crate::naming::deconflict_ident(to_ident(&seed, Case::Pascal), &mut seen),
                        ty,
                    }
                }
            };
            variants.push(variant);
        }
        diagnostics.into_result()?;
        return Ok(variants);
    }

    /// Build a string enum from an OpenAPI string `enum`.
    ///
    /// `x-enum-varnames` / `x-enumNames` override variant identifiers positionally
    /// (in declaration order). The wire value is preserved via `#[serde(rename)]`.
    ///
    /// A repeated value is an error. The second variant would take the same
    /// `rename`, which leaves it unreachable and compiles only with a warning.
    fn string_enum(&self, name: &str, values: &[Option<String>], data: &SchemaData) -> Result<Enum> {
        let varnames = match extension_str_array(data, X_ENUM_VARNAMES, name)? {
            Some(names) => Some(names),
            None => extension_str_array(data, X_ENUM_NAMES, name)?,
        };
        let mut diagnostics = crate::lower::validate::Diagnostics::new();
        let mut variants = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut values_seen = std::collections::HashSet::new();
        for (index, value) in values.iter().flatten().enumerate() {
            if !values_seen.insert(value.as_str()) {
                diagnostics.push(Error::UnsupportedSchema {
                    path: name.to_owned(),
                    reason: format!("the `enum` gives `{value}` more than once"),
                });
                continue;
            }
            let base = match varnames.as_ref().and_then(|names| return names.get(index)) {
                Some(custom) => to_ident(custom, Case::Pascal),
                None => to_ident(value, Case::Pascal),
            };
            let ident = crate::naming::deconflict_ident(base, &mut seen);
            let rename = crate::naming::rename_for(value, &ident);
            variants.push(StringVariant {
                name: ident,
                rename,
                doc: None,
            });
        }
        diagnostics.into_result()?;
        return Ok(Enum {
            name: self.type_name_ident(name),
            doc: doc_of(data),
            deprecated: deprecation_of(data, name)?,
            kind: EnumKind::Strings(variants),
        });
    }

    /// Lower an integer schema that carries an `enum` into a C-like Rust enum.
    ///
    /// A variant takes its name from `x-enum-varnames` when the document gives
    /// one. Otherwise the name comes from the value: `1` gives `Value1`, and
    /// `-1` gives `ValueMinus1`.
    ///
    /// A repeated value is an error, because two variants cannot share one
    /// discriminant (`E0081`). A value the `format` cannot hold is an error for
    /// the same reason: the literal does not fit the `repr`.
    fn integer_enum(&self, name: &str, values: &[Option<i64>], repr: &RustType, data: &SchemaData) -> Result<Enum> {
        let varnames = match extension_str_array(data, X_ENUM_VARNAMES, name)? {
            Some(names) => Some(names),
            None => extension_str_array(data, X_ENUM_NAMES, name)?,
        };
        let mut diagnostics = crate::lower::validate::Diagnostics::new();
        let mut variants = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut values_seen = std::collections::HashSet::new();
        for (index, value) in values.iter().flatten().enumerate() {
            if !values_seen.insert(*value) {
                diagnostics.push(Error::UnsupportedSchema {
                    path: name.to_owned(),
                    reason: format!("the `enum` gives `{value}` more than once"),
                });
                continue;
            }
            if !fits_repr(*value, repr) {
                diagnostics.push(Error::UnsupportedSchema {
                    path: name.to_owned(),
                    reason: format!("the `enum` gives `{value}`, which `{}` cannot hold", repr_name(repr)),
                });
                continue;
            }
            let base = match varnames.as_ref().and_then(|names| return names.get(index)) {
                Some(custom) => to_ident(custom, Case::Pascal),
                None => to_ident(&integer_variant_name(*value), Case::Pascal),
            };
            variants.push(IntegerVariant {
                name: crate::naming::deconflict_ident(base, &mut seen),
                value: *value,
                doc: None,
            });
        }
        diagnostics.into_result()?;
        return Ok(Enum {
            name: self.type_name_ident(name),
            doc: doc_of(data),
            deprecated: deprecation_of(data, name)?,
            kind: EnumKind::Integers {
                repr: repr.clone(),
                variants,
            },
        });
    }

    /// Resolve a property/items/additionalProperties schema reference to a type,
    /// hoisting inline named types into `self.extra` as needed.
    fn type_from_schema_ref(&mut self, hint: &str, schema: &ReferenceOr<Box<Schema>>) -> Result<RustType> {
        match schema {
            ReferenceOr::Reference { reference } => {
                let target = self.schema_ref_target(reference, "a property")?;
                return Ok(RustType::Named(target));
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
                let target = self.schema_ref_target(reference, "additionalProperties")?;
                return Ok(RustType::Named(target));
            }
            ReferenceOr::Item(schema) => {
                let ty = self.type_from_schema(hint, schema)?;
                return Ok(ty);
            }
        }
    }

    /// Map an inline schema to a Rust type, hoisting composite inline schemas
    /// into named items. Bounds inline nesting via [`MAX_SCHEMA_DEPTH`] so a
    /// pathological spec errors cleanly instead of exhausting the stack.
    fn type_from_schema(&mut self, hint: &str, schema: &Schema) -> Result<RustType> {
        if self.depth >= MAX_SCHEMA_DEPTH {
            return Err(Error::SchemaDepthExceeded {
                path: hint.to_owned(),
                limit: MAX_SCHEMA_DEPTH,
            });
        }
        self.depth += 1;
        let result = self.type_from_schema_inner(hint, schema);
        self.depth -= 1;
        return result;
    }

    fn type_from_schema_inner(&mut self, hint: &str, schema: &Schema) -> Result<RustType> {
        let data = &schema.schema_data;
        if let Some(verbatim) = extension_str(data, X_RUST_TYPE, hint)? {
            return verbatim_type(data, verbatim, hint);
        }

        let ty = match &schema.schema_kind {
            SchemaKind::Type(Type::String(st)) if !st.enumeration.is_empty() => {
                let enom = self.string_enum(hint, &st.enumeration, data)?;
                self.extra.push(Item::Enum(enom));
                RustType::Named(hint.to_owned())
            }
            SchemaKind::Type(Type::String(st)) => string_format_type(&st.format),
            SchemaKind::Type(Type::Integer(it)) if !it.enumeration.is_empty() => {
                let repr = integer_format_type(&it.format);
                let enom = self.integer_enum(hint, &it.enumeration, &repr, data)?;
                self.extra.push(Item::Enum(enom));
                RustType::Named(hint.to_owned())
            }
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
                if let Some(ty) = self.collapse_single_all_of(hint, all_of)? {
                    ty
                } else {
                    let strukt = self.merge_all_of(hint, all_of, data)?;
                    self.extra.push(Item::Struct(strukt));
                    RustType::Named(hint.to_owned())
                }
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
pub(crate) fn string_format_type(format: &VariantOrUnknownOrEmpty<StringFormat>) -> RustType {
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

/// Whether an integer enum value fits the `repr` its `format` chooses.
fn fits_repr(value: i64, repr: &RustType) -> bool {
    if matches!(*repr, RustType::I32) {
        return i32::try_from(value).is_ok();
    }
    return true;
}

/// The Rust name of an integer enum `repr`, for a diagnostic.
fn repr_name(repr: &RustType) -> &'static str {
    if matches!(*repr, RustType::I32) {
        return "i32";
    }
    return "i64";
}

/// The default name for an integer enum variant, from its value.
fn integer_variant_name(value: i64) -> String {
    if value < 0 {
        return format!("value_minus_{}", value.unsigned_abs());
    }
    return format!("value_{value}");
}

/// The name an inline union member gives its variant, if it gives one at all.
///
/// `x-rust-name` names any member. Without it, only a member that hoists no type
/// of its own can be named, and the type it holds gives that name.
///
/// A member that hoists — an object, a list, a map, an `enum` — needs a name for
/// the hoisted type as well as for the variant. The position would give one, but
/// a position carries no meaning and moves when the document changes, so the
/// author gives the name instead.
fn inline_variant_seed(schema: &Schema, at: &str) -> Result<Option<String>> {
    if let Some(custom) = extension_str(&schema.schema_data, X_RUST_NAME, at)? {
        return Ok(Some(custom.to_owned()));
    }
    let Some(ty) = non_hoisting_type(schema) else {
        return Ok(None);
    };
    return Ok(type_variant_name(&ty).map(str::to_owned));
}

/// The type an inline union member holds when it hoists nothing.
///
/// `None` means the member hoists a type of its own, which then needs a name.
fn non_hoisting_type(schema: &Schema) -> Option<RustType> {
    return match &schema.schema_kind {
        SchemaKind::Type(Type::String(st)) if st.enumeration.is_empty() => Some(string_format_type(&st.format)),
        SchemaKind::Type(Type::Integer(it)) if it.enumeration.is_empty() => Some(integer_format_type(&it.format)),
        SchemaKind::Type(Type::Number(_)) => Some(RustType::F64),
        SchemaKind::Type(Type::Boolean(_)) => Some(RustType::Bool),
        _ => None,
    };
}

/// The variant name a type gives, for a union member that hoists nothing.
///
/// A union cannot hold one type twice, so these names stay unique.
fn type_variant_name(ty: &RustType) -> Option<&'static str> {
    return match ty {
        RustType::Bool => Some("Bool"),
        RustType::I32 => Some("I32"),
        RustType::I64 => Some("I64"),
        RustType::F64 => Some("F64"),
        RustType::String => Some("String"),
        RustType::Date => Some("Date"),
        RustType::DateTime => Some("DateTime"),
        RustType::Uuid => Some("Uuid"),
        RustType::Bytes => Some("Bytes"),
        _ => None,
    };
}

/// Reject a union that holds one type more than once.
///
/// The emitted enum is `#[serde(untagged)]`. Serde reads the variants in order
/// and takes the first that fits, so a repeated type makes the later variant
/// unreachable. A value built with that variant comes back as the earlier one,
/// which changes the value and reports nothing.
fn check_variant_types(name: &str, variants: &[UnionVariant]) -> Result<()> {
    let mut diagnostics = crate::lower::validate::Diagnostics::new();
    for (index, variant) in variants.iter().enumerate() {
        let Some(earlier) = variants.iter().take(index).find(|other| return other.ty == variant.ty) else {
            continue;
        };
        diagnostics.push(Error::UnsupportedSchema {
            path: name.to_owned(),
            reason: format!(
                "the union holds `{}` twice, as `{}` and as `{}`",
                variant.ty.label(),
                earlier.name.logical(),
                variant.name.logical()
            ),
        });
    }
    return diagnostics.into_result();
}

/// Map an integer `format` to a Rust type.
pub(crate) fn integer_format_type(format: &VariantOrUnknownOrEmpty<IntegerFormat>) -> RustType {
    let ty = match format {
        VariantOrUnknownOrEmpty::Item(IntegerFormat::Int32) => RustType::I32,
        VariantOrUnknownOrEmpty::Item(IntegerFormat::Int64) => RustType::I64,
        VariantOrUnknownOrEmpty::Unknown(_) | VariantOrUnknownOrEmpty::Empty => RustType::I64,
    };
    return ty;
}

/// Extract a string-valued extension (for example `x-rust-type`) from schema data.
fn extension_str<'a>(data: &'a SchemaData, key: &str, at: &str) -> Result<Option<&'a str>> {
    return crate::lower::extension::str_value(&data.extensions, key, at);
}

/// Extract a boolean-valued extension (for example `x-omitempty`) from schema data.
fn extension_bool(data: &SchemaData, key: &str, at: &str) -> Result<Option<bool>> {
    return crate::lower::extension::bool_value(&data.extensions, key, at);
}

/// Extract an integer-valued extension (for example `x-order`) from schema data.
fn extension_i64(data: &SchemaData, key: &str, at: &str) -> Result<Option<i64>> {
    return crate::lower::extension::i64_value(&data.extensions, key, at);
}

/// The `x-order` value of a property, if it carries one (only inline schemas can).
fn prop_order(prop: &ReferenceOr<Box<Schema>>, at: &str) -> Result<Option<i64>> {
    return match prop {
        ReferenceOr::Item(schema) => extension_i64(&schema.schema_data, X_ORDER, at),
        ReferenceOr::Reference { .. } => Ok(None),
    };
}

/// Order fields by their `x-order` (ascending), keeping fields without one in
/// their original declaration order after the ordered ones (a stable sort with
/// unordered fields treated as coming last).
fn sort_by_order(mut fields: Vec<(Option<i64>, Field)>) -> Vec<Field> {
    fields.sort_by_key(|(order, _)| {
        return order.unwrap_or(i64::MAX);
    });
    return fields.into_iter().map(|(_, field)| return field).collect();
}

/// Extract a string-array extension (for example `x-enum-varnames`).
fn extension_str_array<'a>(data: &'a SchemaData, key: &str, at: &str) -> Result<Option<Vec<&'a str>>> {
    return crate::lower::extension::str_list_value(&data.extensions, key, at);
}

/// The trait names `x-rust-derive` accepts, in the order the derive list emits
/// them. Only these three, because they are the only traits the generator derives
/// on a model without being asked. A serde trait is decided by the direction the
/// API uses the model in, which [`crate::emit::usage`] computes and no
/// specification overrides.
const FOREIGN_DERIVE_NAMES: [&str; 3] = ["Debug", "Clone", "PartialEq"];

/// Read `x-rust-derive` into a [`ForeignDerives`].
///
/// The value lists the traits the target **does** implement, so an absent key
/// means all three and an empty list means none. Listing what is present rather
/// than what is missing keeps the specification readable: a reader sees the
/// target's capability and not a double negative.
///
/// An unknown trait name is an error and not an ignored key. The whole point of
/// the extension is to stop a bound the author cannot satisfy, so a misspelled
/// `Parialeq` that silently claims nothing would give exactly the compile error
/// the author wrote the key to avoid, with nothing pointing at the typo.
fn foreign_derives_of(data: &SchemaData, path: &str) -> Result<ForeignDerives> {
    let Some(value) = data.extensions.get(X_RUST_DERIVE) else {
        return Ok(ForeignDerives::default());
    };
    let Some(array) = value.as_array() else {
        return Err(Error::UnsupportedSchema {
            path: path.to_owned(),
            reason: format!("`{X_RUST_DERIVE}` must be a list of trait names, for example `[Debug, Clone]`"),
        });
    };

    let mut derives = ForeignDerives {
        debug: false,
        clone: false,
        partial_eq: false,
    };
    for entry in array {
        let Some(name) = entry.as_str() else {
            return Err(Error::UnsupportedSchema {
                path: path.to_owned(),
                reason: format!("every `{X_RUST_DERIVE}` entry must be a trait name written as a string"),
            });
        };
        match name {
            "Debug" => derives.debug = true,
            "Clone" => derives.clone = true,
            "PartialEq" => derives.partial_eq = true,
            other => {
                let known = FOREIGN_DERIVE_NAMES.join(", ");
                return Err(Error::UnsupportedSchema {
                    path: path.to_owned(),
                    reason: format!("`{X_RUST_DERIVE}` does not accept `{other}`. It accepts only {known}"),
                });
            }
        }
    }
    return Ok(derives);
}

/// Lower an `x-rust-type` target into a [`RustType::Verbatim`], reading its
/// `x-rust-derive` alongside. Both keys sit on one schema, so they are read
/// together and neither site has to remember the other exists.
fn verbatim_type(data: &SchemaData, verbatim: &str, path: &str) -> Result<RustType> {
    return Ok(RustType::Verbatim {
        text: verbatim.to_owned(),
        derives: foreign_derives_of(data, path)?,
    });
}

/// Derive a `#[deprecated]` annotation from `deprecated: true` and an optional
/// `x-deprecated-reason` note. Returns `None` unless the schema is deprecated,
/// so a lone `x-deprecated-reason` is a no-op (matching `oapi-codegen`).
fn deprecation_of(data: &SchemaData, at: &str) -> Result<Option<Deprecation>> {
    if !data.deprecated {
        return Ok(None);
    }
    let note = extension_str(data, X_DEPRECATED_REASON, at)?.map(str::to_owned);
    return Ok(Some(Deprecation { note }));
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
        let module = lower_models(&spec).expect("map schemas");
        return crate::emit::emit_module(&module, None).expect("emit module");
    }

    /// Lower every schema with no configured suffix. None of the specs below
    /// holds a name collision, so the resolved names need no further check.
    fn lower_models(spec: &Spec) -> Result<Module> {
        let names = crate::lower::rename::type_renames(spec, None)?;
        return generate_models(spec, &names);
    }

    const PREAMBLE: &str = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\ncomponents:\n  schemas:\n";

    /// A schema of `depth` nested inline arrays terminating in a string, built
    /// programmatically so the depth guard — not the YAML parser's own recursion
    /// limit or a parse-time stack overflow — is what the test exercises.
    fn nested_array_schema(depth: usize) -> Schema {
        let mut kind = SchemaKind::Type(Type::String(Default::default()));
        for _ in 0..depth {
            let items = ReferenceOr::Item(Box::new(Schema {
                schema_data: SchemaData::default(),
                schema_kind: kind,
            }));
            kind = SchemaKind::Type(Type::Array(openapiv3::ArrayType {
                items: Some(items),
                min_items: None,
                max_items: None,
                unique_items: false,
            }));
        }
        return Schema {
            schema_data: SchemaData::default(),
            schema_kind: kind,
        };
    }

    fn spec_with_schema(name: &str, schema: Schema) -> Spec {
        let empty_doc = "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\n";
        let mut doc: openapiv3::OpenAPI = serde_yaml::from_str(empty_doc).expect("parse preamble");
        doc.components
            .get_or_insert_with(Default::default)
            .schemas
            .insert(name.to_owned(), ReferenceOr::Item(schema));
        return Spec::from_parts(doc, PathBuf::from("inline.yaml"));
    }
    // The two tests below bracket the guard boundary exactly: the deepest
    // schema that lowers is `MAX_SCHEMA_DEPTH - 1` levels, and reaching
    // `MAX_SCHEMA_DEPTH` errors. Any off-by-one in the guard breaks one of them.

    #[test]
    fn schema_at_the_depth_limit_errors_instead_of_overflowing() {
        let spec = spec_with_schema("Deep", nested_array_schema(MAX_SCHEMA_DEPTH));
        let err = lower_models(&spec).expect_err("reaching the limit should hit the depth guard");
        assert!(
            matches!(err, Error::SchemaDepthExceeded { limit, .. } if limit == MAX_SCHEMA_DEPTH),
            "expected SchemaDepthExceeded, got {err:?}"
        );
    }

    #[test]
    fn schema_just_under_the_depth_limit_still_lowers() {
        let spec = spec_with_schema("Deep", nested_array_schema(MAX_SCHEMA_DEPTH - 1));
        lower_models(&spec).expect("just under the limit should lower cleanly");
    }

    /// Lower a spec whose one schema carries an `x-rust-derive` and return the
    /// error, for the shapes the extension rejects.
    fn derive_error(value: &str) -> Error {
        let yaml = format!(
            "{PREAMBLE}    Target:\n      type: string\n      x-rust-type: crate::Foreign\n      x-rust-derive: {value}\n"
        );
        let doc: openapiv3::OpenAPI = serde_yaml::from_str(&yaml).expect("parse spec");
        let spec = Spec::from_parts(doc, PathBuf::from("inline.yaml"));
        return lower_models(&spec).expect_err("the extension should reject this value");
    }

    #[test]
    fn absent_x_rust_derive_claims_every_trait() {
        // The default every specification written before the extension existed
        // relies on. A model reaching the target keeps all three traits.
        let out = emit_yaml(&format!(
            "{PREAMBLE}    Holder:\n      type: object\n      required: [value]\n      properties:\n        value:\n          type: string\n          x-rust-type: crate::Foreign\n"
        ));
        assert!(
            out.contains("#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]"),
            "absent key should change nothing:\n{out}"
        );
    }

    #[test]
    fn empty_x_rust_derive_drops_every_trait() {
        let out = emit_yaml(&format!(
            "{PREAMBLE}    Holder:\n      type: object\n      required: [value]\n      properties:\n        value:\n          type: string\n          x-rust-type: crate::Foreign\n          x-rust-derive: []\n"
        ));
        assert!(
            out.contains("#[derive(serde::Serialize, serde::Deserialize)]"),
            "an empty list claims nothing, so only the serde derives remain:\n{out}"
        );
    }

    #[test]
    fn partial_x_rust_derive_keeps_only_the_listed_traits() {
        let out = emit_yaml(&format!(
            "{PREAMBLE}    Holder:\n      type: object\n      required: [value]\n      properties:\n        value:\n          type: string\n          x-rust-type: crate::Foreign\n          x-rust-derive: [Debug, PartialEq]\n"
        ));
        assert!(
            out.contains("#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]"),
            "Clone was not listed, so it is dropped:\n{out}"
        );
    }

    #[test]
    fn x_rust_derive_that_is_not_a_list_is_rejected() {
        let err = derive_error("Debug");
        assert!(
            matches!(&err, Error::UnsupportedSchema { reason, .. } if reason.contains("must be a list")),
            "expected a list-shape error, got {err:?}"
        );
    }

    #[test]
    fn x_rust_derive_entry_that_is_not_a_string_is_rejected() {
        let err = derive_error("[7]");
        assert!(
            matches!(&err, Error::UnsupportedSchema { reason, .. } if reason.contains("written as a string")),
            "expected a string-entry error, got {err:?}"
        );
    }

    #[test]
    fn misspelled_trait_name_is_an_error_and_not_an_ignored_key() {
        // Silently accepting `Parialeq` would claim nothing and produce exactly the
        // compile error the author wrote the key to prevent, with nothing pointing
        // at the typo.
        let err = derive_error("[Parialeq]");
        assert!(
            matches!(&err, Error::UnsupportedSchema { reason, .. } if reason.contains("Parialeq")),
            "expected the unknown name in the error, got {err:?}"
        );
    }

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
