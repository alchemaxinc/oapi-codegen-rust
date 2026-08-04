//! Giving a recursive type a size by boxing the field that closes the cycle.
//!
//! A schema may refer to itself, directly or through other schemas. Lowered
//! as written, `Node { child: Node }` is a type that holds itself, and rustc
//! rejects it with `E0072`. The fix rustc itself suggests is a `Box`, so this
//! pass inserts one.
//!
//! # What counts as holding a type
//!
//! A field holds its type when the size of that type counts towards the size of
//! the struct. `Vec<T>` and `HashMap<String, T>` keep their elements on the
//! heap, so they hold nothing and already break a cycle. `Option<T>` stores its
//! `T` inline, so `Option<Node>` inside `Node` is just as infinite as `Node`.
//! That last point is easy to get wrong: making a recursive property optional
//! does not fix anything.
//!
//! # Which edge gets the box
//!
//! Every edge on a cycle, and not one chosen edge. Boxing a single edge is
//! enough for rustc, but the choice would fall out of item order, so `A` and
//! `B` that refer to each other would get one box on whichever the walk met
//! first. Boxing both states the same fact about both types.
//!
//! An alias holds its target and offers nothing to box, so a cycle made only of
//! aliases is [`Error::RecursiveAlias`] instead.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::error::Error;
use crate::error::Result;
use crate::ir::EnumKind;
use crate::ir::Item;
use crate::ir::Module;
use crate::ir::RustType;
use crate::naming::Case;
use crate::naming::to_ident;

/// Box every field and variant of `module` that closes a type cycle.
///
/// Fails when a cycle runs only through aliases, because a `Box` there still
/// expands forever.
pub fn box_recursive_types(module: &mut Module) -> Result<()> {
    let graph = Graph::of(module);
    graph.check_alias_cycles()?;

    for item in &mut module.items {
        let owner = canonical(item.name());
        match item {
            Item::Struct(strukt) => {
                for field in &mut strukt.fields {
                    box_held(&mut field.ty, &owner, &graph);
                }
            }
            Item::Enum(enumeration) => {
                if let EnumKind::Union(variants) = &mut enumeration.kind {
                    for variant in variants {
                        box_held(&mut variant.ty, &owner, &graph);
                    }
                }
            }
            // An alias offers nothing to box. `check_alias_cycles` has already
            // rejected the only cycle that could reach one.
            Item::Alias(_) => {}
        }
    }
    return Ok(());
}

/// Which named types each item holds, keyed by canonical name.
struct Graph {
    /// Held types per item. An alias entry is kept apart so a cycle can be told
    /// to run through aliases alone.
    held: BTreeMap<String, BTreeSet<String>>,
    /// The items that are aliases.
    aliases: BTreeSet<String>,
}

impl Graph {
    /// Build the holding graph of `module`.
    fn of(module: &Module) -> Self {
        let mut held = BTreeMap::new();
        let mut aliases = BTreeSet::new();
        for item in &module.items {
            let name = canonical(item.name());
            let mut targets = BTreeSet::new();
            match item {
                Item::Struct(strukt) => {
                    for field in &strukt.fields {
                        collect_held(&field.ty, &mut targets);
                    }
                }
                Item::Enum(enumeration) => {
                    if let EnumKind::Union(variants) = &enumeration.kind {
                        for variant in variants {
                            collect_held(&variant.ty, &mut targets);
                        }
                    }
                }
                Item::Alias(alias) => {
                    aliases.insert(name.clone());
                    collect_held(&alias.ty, &mut targets);
                }
            }
            held.insert(name, targets);
        }
        return Self { held, aliases };
    }

    /// Whether `from` holds `to`, directly or through other items.
    fn reaches(&self, from: &str, to: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut worklist = vec![from.to_owned()];
        while let Some(current) = worklist.pop() {
            let Some(targets) = self.held.get(&current) else {
                continue;
            };
            for target in targets {
                if target == to {
                    return true;
                }
                if seen.insert(target.clone()) {
                    worklist.push(target.clone());
                }
            }
        }
        return false;
    }

    /// Reject a cycle whose every member is an alias.
    ///
    /// Such a cycle has no field and no variant to box, and `type A = Box<B>`
    /// with `type B = Box<A>` still expands forever.
    fn check_alias_cycles(&self) -> Result<()> {
        for name in &self.aliases {
            let Some(cycle) = self.alias_cycle_from(name) else {
                continue;
            };
            return Err(Error::RecursiveAlias {
                cycle,
                hint: "Give one of these schemas `type: object` with properties, so the generator emits a struct it \
                       can box, or break the chain of `$ref`s."
                    .to_owned(),
            });
        }
        return Ok(());
    }

    /// The cycle of aliases starting at `start`, when one exists.
    ///
    /// The walk only steps through aliases, so a chain that leaves the aliases
    /// and comes back through a struct is not reported here. The struct on that
    /// chain gets a box instead.
    fn alias_cycle_from(&self, start: &str) -> Option<Vec<String>> {
        let mut path = vec![start.to_owned()];
        let mut current = start.to_owned();
        // Each alias holds at most one named type, so the walk never branches
        // and stops after it has seen every alias once.
        for _ in 0..=self.aliases.len() {
            let next = self
                .held
                .get(&current)
                .and_then(|targets| return targets.iter().next())
                .filter(|target| return self.aliases.contains(*target))
                .cloned()?;
            if next == start {
                path.push(next);
                return Some(path);
            }
            path.push(next.clone());
            current = next;
        }
        return None;
    }
}

/// Add every named type that `ty` holds to `out`.
///
/// `Vec` and `Map` put their element on the heap, so the walk stops there.
fn collect_held(ty: &RustType, out: &mut BTreeSet<String>) {
    match ty {
        RustType::Named(name) => {
            out.insert(canonical(name));
        }
        RustType::Option(inner) | RustType::Boxed(inner) => collect_held(inner, out),
        _ => {}
    }
}

/// Box the named types inside `ty` that hold `owner` back.
fn box_held(ty: &mut RustType, owner: &str, graph: &Graph) {
    match ty {
        RustType::Named(name) => {
            let target = canonical(name);
            if target == owner || graph.reaches(&target, owner) {
                let inner = std::mem::replace(ty, RustType::Bool);
                *ty = RustType::Boxed(Box::new(inner));
            }
        }
        RustType::Option(inner) => box_held(inner, owner, graph),
        _ => {}
    }
}

/// The name an item is known by in the graph, matching how the emitter names it.
fn canonical(name: &str) -> String {
    return to_ident(name, Case::Pascal).logical().to_owned();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Alias;
    use crate::ir::Enum;
    use crate::ir::Field;
    use crate::ir::Struct;
    use crate::ir::UnionVariant;

    /// A struct of one field, which is the shape every case below needs.
    fn one_field(name: &str, field: &str, ty: RustType) -> Item {
        return Item::Struct(Struct {
            name: to_ident(name, Case::Pascal),
            doc: None,
            deprecated: None,
            fields: vec![Field {
                name: to_ident(field, Case::Snake),
                rename: None,
                doc: None,
                deprecated: None,
                ty,
                required: true,
                omit_empty: None,
                serde_skip: false,
            }],
            additional_properties: None,
            deny_unknown_fields: false,
        });
    }

    /// The type of the first field of the first item named `name`.
    fn field_type(module: &Module, name: &str) -> RustType {
        for item in &module.items {
            if let Item::Struct(strukt) = item
                && strukt.name.logical() == name
            {
                return strukt.fields[0].ty.clone();
            }
        }
        panic!("no struct named `{name}`");
    }

    fn named(name: &str) -> RustType {
        return RustType::Named(name.to_owned());
    }

    fn boxed(inner: RustType) -> RustType {
        return RustType::Boxed(Box::new(inner));
    }

    /// A field is boxed when, and only when, its type holds the owner back
    /// without going through the heap.
    #[test]
    fn only_a_field_that_holds_its_owner_is_boxed() {
        let cases: &[(&str, RustType, RustType)] = &[
            ("direct", named("Node"), boxed(named("Node"))),
            (
                "through an option",
                RustType::Option(Box::new(named("Node"))),
                RustType::Option(Box::new(boxed(named("Node")))),
            ),
            (
                "through a vec",
                RustType::Vec(Box::new(named("Node"))),
                RustType::Vec(Box::new(named("Node"))),
            ),
            (
                "through a map",
                RustType::Map(Box::new(named("Node"))),
                RustType::Map(Box::new(named("Node"))),
            ),
            ("a scalar", RustType::String, RustType::String),
        ];

        for (label, input, want) in cases {
            let mut module = Module {
                items: vec![one_field("Node", "child", input.clone())],
            };
            box_recursive_types(&mut module).expect("no alias cycle in this module");
            assert_eq!(field_type(&module, "Node"), *want, "self-reference {label}");
        }
    }

    /// Both sides of a two-type cycle are boxed, so neither depends on the order
    /// the items happen to sit in.
    #[test]
    fn both_sides_of_a_mutual_cycle_are_boxed() {
        let mut module = Module {
            items: vec![
                one_field("Parent", "child", named("Kid")),
                one_field("Kid", "parent", named("Parent")),
            ],
        };
        box_recursive_types(&mut module).expect("no alias cycle in this module");
        assert_eq!(field_type(&module, "Parent"), boxed(named("Kid")));
        assert_eq!(field_type(&module, "Kid"), boxed(named("Parent")));
    }

    /// A type that a cycle merely points at is not part of the cycle, so it
    /// keeps its plain field.
    #[test]
    fn a_type_the_cycle_only_points_at_is_left_alone() {
        let mut module = Module {
            items: vec![
                one_field("Node", "child", named("Node")),
                one_field("Holder", "node", named("Node")),
            ],
        };
        box_recursive_types(&mut module).expect("no alias cycle in this module");
        assert_eq!(field_type(&module, "Holder"), named("Node"));
    }

    /// A union variant holds its type the way a field does, so it is boxed too.
    #[test]
    fn a_union_variant_that_holds_its_own_enum_is_boxed() {
        let mut module = Module {
            items: vec![Item::Enum(Enum {
                name: to_ident("Expression", Case::Pascal),
                doc: None,
                deprecated: None,
                kind: EnumKind::Union(vec![
                    UnionVariant {
                        name: to_ident("Text", Case::Pascal),
                        ty: RustType::String,
                    },
                    UnionVariant {
                        name: to_ident("Nested", Case::Pascal),
                        ty: named("Expression"),
                    },
                ]),
            })],
        };
        box_recursive_types(&mut module).expect("no alias cycle in this module");
        let Item::Enum(enumeration) = &module.items[0] else {
            panic!("the item is an enum");
        };
        let EnumKind::Union(variants) = &enumeration.kind else {
            panic!("the enum is a union");
        };
        assert_eq!(variants[0].ty, RustType::String);
        assert_eq!(variants[1].ty, boxed(named("Expression")));
    }

    /// An alias offers nothing to box, so a cycle running through one is broken
    /// at the struct field instead.
    #[test]
    fn a_cycle_through_an_alias_is_boxed_at_the_struct() {
        let mut module = Module {
            items: vec![
                Item::Alias(Alias {
                    name: to_ident("Wrapper", Case::Pascal),
                    doc: None,
                    deprecated: None,
                    ty: named("Holder"),
                }),
                one_field("Holder", "wrapped", named("Wrapper")),
            ],
        };
        box_recursive_types(&mut module).expect("this cycle holds a struct, so it is not alias-only");
        assert_eq!(field_type(&module, "Holder"), boxed(named("Wrapper")));
    }

    /// A cycle made only of aliases has nowhere to put a box, so it is an error.
    #[test]
    fn an_alias_only_cycle_is_rejected() {
        let alias = |name: &str, target: &str| {
            return Item::Alias(Alias {
                name: to_ident(name, Case::Pascal),
                doc: None,
                deprecated: None,
                ty: named(target),
            });
        };
        let mut module = Module {
            items: vec![alias("Loop", "Ring"), alias("Ring", "Loop")],
        };
        let outcome = box_recursive_types(&mut module);
        assert!(
            matches!(outcome, Err(Error::RecursiveAlias { .. })),
            "a cycle of aliases must be rejected, and gave: {outcome:?}",
        );
    }

    /// An alias that refers to itself is the same problem with one member.
    #[test]
    fn a_self_referencing_alias_is_rejected() {
        let mut module = Module {
            items: vec![Item::Alias(Alias {
                name: to_ident("Loop", Case::Pascal),
                doc: None,
                deprecated: None,
                ty: named("Loop"),
            })],
        };
        assert!(matches!(
            box_recursive_types(&mut module),
            Err(Error::RecursiveAlias { .. })
        ));
    }

    /// A module with no cycle keeps every type exactly as it was.
    #[test]
    fn a_module_without_a_cycle_is_unchanged() {
        let mut module = Module {
            items: vec![
                one_field("Holder", "node", named("Node")),
                one_field("Node", "id", RustType::String),
            ],
        };
        let before = module.clone();
        box_recursive_types(&mut module).expect("no alias cycle in this module");
        assert_eq!(module, before);
    }
}
