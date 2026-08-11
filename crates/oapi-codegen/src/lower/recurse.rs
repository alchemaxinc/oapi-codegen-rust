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
//! the struct. `Vec<T>`, `HashMap<String, T>` and `Box<T>` keep what they hold
//! on the heap, so they hold nothing and already break a cycle. `Option<T>`
//! stores its `T` inline, so `Option<Node>` inside `Node` is just as infinite as
//! `Node`. That last point is easy to get wrong: making a recursive property
//! optional does not fix anything.
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

/// Which items hold which, plus the strongly connected components of that
/// relation.
///
/// An edge `a -> b` means an item `a` holds an item `b` in a position that
/// counts towards its size. Two items sit in one component exactly when each
/// holds the other, so an edge needs a box exactly when its two ends share a
/// component. Reading the components one time keeps the pass linear in the
/// number of edges; asking "does `b` reach `a`" per edge walks the whole graph
/// per edge instead.
struct Graph {
    /// Item names, in module order, indexed by node.
    names: Vec<String>,
    /// Node per item name.
    nodes: BTreeMap<String, usize>,
    /// Held items per node.
    edges: Vec<Vec<usize>>,
    /// Whether each node is an alias, which offers nothing to box.
    is_alias: Vec<bool>,
    /// Component per node.
    component: Vec<usize>,
}

impl Graph {
    /// Build the holding graph of `module` and its components.
    fn of(module: &Module) -> Self {
        let names: Vec<String> = module.items.iter().map(|item| return canonical(item.name())).collect();
        let nodes: BTreeMap<String, usize> = names
            .iter()
            .enumerate()
            .map(|(node, name)| return (name.clone(), node))
            .collect();

        let mut edges = Vec::with_capacity(module.items.len());
        let mut is_alias = Vec::with_capacity(module.items.len());
        for item in &module.items {
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
                Item::Alias(alias) => collect_held(&alias.ty, &mut targets),
            }
            is_alias.push(matches!(item, Item::Alias(_)));
            // A name the module does not declare cannot close a cycle inside it,
            // so it gets no node and no edge.
            edges.push(
                targets
                    .iter()
                    .filter_map(|target| return nodes.get(target).copied())
                    .collect(),
            );
        }

        let component = components(&edges);
        return Self {
            names,
            nodes,
            edges,
            is_alias,
            component,
        };
    }

    /// Whether an edge between `from` and `to` closes a cycle.
    ///
    /// The caller only asks about a pair it holds an edge for, so one shared
    /// component is the same statement as "each reaches the other".
    fn on_a_cycle(&self, from: &str, to: &str) -> bool {
        let (Some(from), Some(to)) = (self.nodes.get(from), self.nodes.get(to)) else {
            return false;
        };
        let (Some(from), Some(to)) = (self.component.get(*from), self.component.get(*to)) else {
            return false;
        };
        return from == to;
    }

    /// Reject a cycle whose every member is an alias.
    ///
    /// Such a cycle has no field and no variant to box, and `type A = Box<B>`
    /// with `type B = Box<A>` still expands forever.
    fn check_alias_cycles(&self) -> Result<()> {
        let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (node, component) in self.component.iter().enumerate() {
            members.entry(*component).or_default().push(node);
        }
        for group in members.values() {
            if !self.is_cyclic(group) || !group.iter().all(|node| return self.is_alias(*node)) {
                continue;
            }
            return Err(Error::RecursiveAlias {
                cycle: self.cycle_through(group),
                hint: "Give one of these schemas `type: object` with properties, so the generator emits a struct it \
                       can box, or break the chain of `$ref`s."
                    .to_owned(),
            });
        }
        return Ok(());
    }

    /// Whether the items of one component refer to each other in a cycle.
    ///
    /// Every node is its own component, so a lone node is only cyclic when it
    /// holds itself.
    fn is_cyclic(&self, group: &[usize]) -> bool {
        if group.len() > 1 {
            return true;
        }
        return group
            .first()
            .is_some_and(|node| return self.edges_of(*node).contains(node));
    }

    /// The items `node` holds. An unknown node holds nothing.
    fn edges_of(&self, node: usize) -> &[usize] {
        return self.edges.get(node).map_or(&[], Vec::as_slice);
    }

    /// Whether `node` is an alias. An unknown node is not.
    fn is_alias(&self, node: usize) -> bool {
        return self.is_alias.get(node).copied().unwrap_or(false);
    }

    /// The name of `node`, empty for an unknown one.
    fn name_of(&self, node: usize) -> String {
        return self.names.get(node).cloned().unwrap_or_default();
    }

    /// Name the members of `group` in the order they refer to each other,
    /// closing on the name the walk started from.
    ///
    /// An alias holds at most one item, so the walk never branches.
    fn cycle_through(&self, group: &[usize]) -> Vec<String> {
        let Some(start) = group.first().copied() else {
            return Vec::new();
        };
        let mut path = vec![self.name_of(start)];
        let mut current = start;
        for _ in 0..group.len() {
            let Some(next) = self.edges_of(current).first().copied() else {
                break;
            };
            path.push(self.name_of(next));
            if next == start {
                break;
            }
            current = next;
        }
        return path;
    }
}

/// The strongly connected component of each node, by Tarjan's algorithm.
///
/// The walk carries its own stack, so a long chain of items cannot overflow the
/// real one. It reads each node and each edge one time.
fn components(edges: &[Vec<usize>]) -> Vec<usize> {
    let mut walk = Walk::over(edges.len());
    // Each entry is a node and how many of its edges the walk has taken.
    let mut work: Vec<(usize, usize)> = Vec::new();

    for root in 0..edges.len() {
        if walk.is_open_or_done(root) {
            continue;
        }
        walk.open(root);
        work.push((root, 0));

        while let Some(&(node, taken)) = work.last() {
            let next = edges.get(node).and_then(|held| return held.get(taken)).copied();
            if let Some(next) = next {
                if let Some(entry) = work.last_mut() {
                    entry.1 += 1;
                }
                walk.step(node, next, &mut work);
                continue;
            }

            work.pop();
            if let Some(&(parent, _)) = work.last() {
                walk.carry_up(parent, node);
            }
            walk.close(node);
        }
    }
    return walk.component;
}

/// The per-node bookkeeping of [`components`].
///
/// Every method reads a node through `get`, because the workspace denies
/// `indexing_slicing`. A node outside the graph cannot arise: each one comes
/// from `0..edges.len()` or from an edge, and an edge is built from a node that
/// the module declares.
struct Walk {
    /// The order the walk reached each node in, or [`Walk::UNREACHED`].
    order: Vec<usize>,
    /// The oldest order each node can reach while the walk holds it open.
    lowest: Vec<usize>,
    /// Whether each node is on [`Walk::path`].
    open: Vec<bool>,
    /// The component of each node, filled in as each component closes.
    component: Vec<usize>,
    /// The nodes the walk holds open, oldest first.
    path: Vec<usize>,
    /// The order to give the next node the walk reaches.
    next_order: usize,
    /// The number to give the next component that closes.
    next_component: usize,
}

impl Walk {
    /// The order of a node the walk has not reached.
    const UNREACHED: usize = usize::MAX;

    /// Bookkeeping for a graph of `count` nodes.
    fn over(count: usize) -> Self {
        return Self {
            order: vec![Self::UNREACHED; count],
            lowest: vec![0; count],
            open: vec![false; count],
            component: vec![0; count],
            path: Vec::new(),
            next_order: 0,
            next_component: 0,
        };
    }

    /// Whether the walk has already reached `node`.
    fn is_open_or_done(&self, node: usize) -> bool {
        return self.order_of(node) != Self::UNREACHED;
    }

    /// The order the walk reached `node` in.
    fn order_of(&self, node: usize) -> usize {
        return self.order.get(node).copied().unwrap_or(Self::UNREACHED);
    }

    /// The oldest order `node` can reach.
    fn lowest_of(&self, node: usize) -> usize {
        return self.lowest.get(node).copied().unwrap_or(Self::UNREACHED);
    }

    /// Give `node` its order and hold it open.
    fn open(&mut self, node: usize) {
        if let Some(order) = self.order.get_mut(node) {
            *order = self.next_order;
        }
        if let Some(lowest) = self.lowest.get_mut(node) {
            *lowest = self.next_order;
        }
        if let Some(open) = self.open.get_mut(node) {
            *open = true;
        }
        self.next_order += 1;
        self.path.push(node);
    }

    /// Take the edge from `node` to `next`.
    ///
    /// An unreached `next` is opened and queued on `work`. A `next` the walk
    /// still holds open closes a loop, so `node` inherits its order.
    fn step(&mut self, node: usize, next: usize, work: &mut Vec<(usize, usize)>) {
        if !self.is_open_or_done(next) {
            self.open(next);
            work.push((next, 0));
            return;
        }
        if self.open.get(next).copied().unwrap_or(false) {
            self.lower(node, self.order_of(next));
        }
    }

    /// Carry what `node` reached up to the `parent` the walk came from.
    fn carry_up(&mut self, parent: usize, node: usize) {
        self.lower(parent, self.lowest_of(node));
    }

    /// Lower the oldest order `node` can reach to `order`, when that is older.
    fn lower(&mut self, node: usize, order: usize) {
        if let Some(lowest) = self.lowest.get_mut(node) {
            *lowest = (*lowest).min(order);
        }
    }

    /// Close the component `node` roots, when it roots one.
    ///
    /// A node that reaches nothing older than itself roots a component holding
    /// every node opened since.
    fn close(&mut self, node: usize) {
        if self.lowest_of(node) != self.order_of(node) {
            return;
        }
        while let Some(member) = self.path.pop() {
            if let Some(open) = self.open.get_mut(member) {
                *open = false;
            }
            if let Some(component) = self.component.get_mut(member) {
                *component = self.next_component;
            }
            if member == node {
                break;
            }
        }
        self.next_component += 1;
    }
}

/// Add every named type that `ty` holds to `out`.
///
/// `Vec`, `Map` and `Box` put what they hold on the heap, so the walk stops at
/// each of them. `Box` counts here as well as the other two, so a run over a
/// module this pass has already boxed sees the cycle as broken and changes
/// nothing.
fn collect_held(ty: &RustType, out: &mut BTreeSet<String>) {
    match ty {
        RustType::Named(name) => {
            out.insert(canonical(name));
        }
        RustType::Option(inner) => collect_held(inner, out),
        _ => {}
    }
}

/// Box the named types inside `ty` that hold `owner` back.
///
/// A type that holds itself is one node and one component, so it needs no case
/// of its own.
fn box_held(ty: &mut RustType, owner: &str, graph: &Graph) {
    match ty {
        RustType::Named(name) => {
            let target = canonical(name);
            if graph.on_a_cycle(&target, owner) {
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
                default: None,
                constraints: None,
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

    /// An edge that is already boxed breaks the cycle, so nothing else on it is
    /// boxed.
    ///
    /// A `Box` is heap indirection, so a boxed field holds nothing. A walk that
    /// stepped through a `Box` would still see the old cycle and would box a
    /// second edge that needs no box.
    #[test]
    fn an_existing_box_breaks_the_cycle_for_every_other_edge() {
        let mut module = Module {
            items: vec![
                one_field("Parent", "child", boxed(named("Kid"))),
                one_field("Kid", "parent", named("Parent")),
            ],
        };
        box_recursive_types(&mut module).expect("no alias cycle in this module");
        assert_eq!(field_type(&module, "Parent"), boxed(named("Kid")));
        assert_eq!(field_type(&module, "Kid"), named("Parent"));
    }
}
