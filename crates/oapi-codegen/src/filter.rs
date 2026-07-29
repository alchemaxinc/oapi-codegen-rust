//! Spec-level operation and schema filtering.
//!
//! Mirrors `oapi-codegen`'s `output-options` filters: operations are dropped
//! when they fail any active include/exclude **tag** or **operation-id** filter,
//! and component schemas named in `exclude-schemas` are removed before lowering.
//! Filtering runs before pruning, so removing an operation lets the prune pass
//! drop any component schemas it uniquely referenced.

use openapiv3::OpenAPI;
use openapiv3::Operation;
use openapiv3::PathItem;
use openapiv3::ReferenceOr;

use crate::config::OutputOptions;

/// Apply the configured operation and schema filters to `doc` in place.
pub fn apply(doc: &mut OpenAPI, opts: &OutputOptions) {
    filter_operations(doc, opts);
    exclude_schemas(doc, &opts.exclude_schemas);
}

/// Whether any operation-level (tag or operation-id) filter is configured.
fn has_operation_filters(opts: &OutputOptions) -> bool {
    return !opts.include_tags.is_empty()
        || !opts.exclude_tags.is_empty()
        || !opts.include_operation_ids.is_empty()
        || !opts.exclude_operation_ids.is_empty();
}

/// Remove operations that fail any active tag or operation-id filter.
fn filter_operations(doc: &mut OpenAPI, opts: &OutputOptions) {
    if !has_operation_filters(opts) {
        return;
    }
    for (_, entry) in doc.paths.paths.iter_mut() {
        let ReferenceOr::Item(item) = entry else {
            continue;
        };
        for slot in operation_slots(item) {
            let remove = slot.as_ref().is_some_and(|op| {
                return is_filtered_out(op, opts);
            });
            if remove {
                *slot = None;
            }
        }
    }
}

/// Mutable references to every operation slot on a path item, in a stable
/// verb order (`get`, `put`, `post`, `delete`, `options`, `head`, `patch`,
/// `trace`).
fn operation_slots(item: &mut PathItem) -> [&mut Option<Operation>; 8] {
    return [
        &mut item.get,
        &mut item.put,
        &mut item.post,
        &mut item.delete,
        &mut item.options,
        &mut item.head,
        &mut item.patch,
        &mut item.trace,
    ];
}

/// Whether `operation` must be dropped given the configured filters.
///
/// An operation is kept only when it carries none of the excluded tags, carries
/// one of the included tags (when `include-tags` is set), is not an excluded
/// operation-id, and is an included operation-id (when `include-operation-ids`
/// is set) — matching `oapi-codegen`'s sequential exclude-then-include filters.
fn is_filtered_out(operation: &Operation, opts: &OutputOptions) -> bool {
    if !opts.exclude_tags.is_empty()
        && operation.tags.iter().any(|tag| {
            return opts.exclude_tags.contains(tag);
        })
    {
        return true;
    }
    if !opts.include_tags.is_empty()
        && !operation.tags.iter().any(|tag| {
            return opts.include_tags.contains(tag);
        })
    {
        return true;
    }
    let id = operation.operation_id.as_deref();
    if !opts.exclude_operation_ids.is_empty()
        && id.is_some_and(|id| return contains_str(&opts.exclude_operation_ids, id))
    {
        return true;
    }
    if !opts.include_operation_ids.is_empty()
        && !id.is_some_and(|id| return contains_str(&opts.include_operation_ids, id))
    {
        return true;
    }
    return false;
}

/// Whether `values` contains `needle`.
fn contains_str(values: &[String], needle: &str) -> bool {
    return values.iter().any(|value| {
        return value == needle;
    });
}

/// Drop component schemas whose names appear in `exclude`.
fn exclude_schemas(doc: &mut OpenAPI, exclude: &[String]) {
    if exclude.is_empty() {
        return;
    }
    if let Some(components) = doc.components.as_mut() {
        components.schemas.retain(|name, _| {
            return !exclude.contains(name);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `operation_slots` must reference every operation verb openapiv3
    /// recognises: nulling all slots must leave the path item with no
    /// operations. This cross-makes sure that our hand-listed slots against openapiv3's
    /// authoritative `PathItem::iter`, catching drift if a verb is ever added.
    #[test]
    fn operation_slots_cover_every_verb() {
        let populated = || {
            return Some(Operation::default());
        };
        let mut item = PathItem {
            get: populated(),
            put: populated(),
            post: populated(),
            delete: populated(),
            options: populated(),
            head: populated(),
            patch: populated(),
            trace: populated(),
            ..Default::default()
        };
        assert_eq!(
            item.iter().count(),
            operation_slots(&mut item).len(),
            "every populated verb should map to one slot"
        );
        for slot in operation_slots(&mut item) {
            *slot = None;
        }
        assert_eq!(
            item.iter().count(),
            0,
            "operation_slots must cover every verb reported by openapiv3's PathItem::iter",
        );
    }
}
