//! Collection of semantic problems into one report.
//!
//! The generator stops on an ambiguity and does not guess. See the "Design
//! differences" section of the README. A spec with several independent problems
//! therefore costs one run for each problem. [`Diagnostics`] collects independent
//! problems, so one run reports all of them.
//!
//! Two rules keep this report useful.
//!
//! * The collector holds independent problems only. Some checks produce a result
//!   that the rest of the lowering pass needs. An unresolved `$ref` is one
//!   example. Such a check must still return at once. A run that continues past
//!   it reports later problems that are only effects of the first one.
//! * One problem is reported as itself. [`Diagnostics::into_result`] returns the
//!   single [`Error`] unchanged. It builds an [`Error::Validation`] for two or
//!   more problems only. A caller that matches one variant still works, and the
//!   message for one problem carries no count.

use crate::error::Error;
use crate::error::Result;

/// A collector of independent semantic problems. One run reports every problem
/// that it finds and does not stop at the first one.
///
/// Add a problem with [`Diagnostics::push`]. Add the result of a fallible check
/// with [`Diagnostics::check`]. End with [`Diagnostics::into_result`].
#[derive(Debug, Default)]
pub struct Diagnostics {
    problems: Vec<Error>,
}

impl Diagnostics {
    /// A collector with no problems recorded.
    pub fn new() -> Self {
        return Self { problems: Vec::new() };
    }

    /// Record a problem and continue.
    ///
    /// A problem that is itself a report opens and adds its problems one by one.
    /// A check can collect on its own and give a report back, and a caller that
    /// collects again would otherwise nest one report inside another. The reader
    /// then gets a count that hides most of the list. [`Error::Validation`]
    /// therefore holds leaf problems only, at one level.
    pub fn push(&mut self, problem: Error) {
        match problem {
            Error::Validation { problems } => self.problems.extend(problems),
            leaf => self.problems.push(leaf),
        }
    }

    /// Record the error from a failed check and continue.
    ///
    /// Use this method for a check that returns a `Result`. That check keeps one
    /// signature. A caller can then run it alone, or as part of this collection
    /// pass.
    pub fn check(&mut self, outcome: Result<()>) {
        if let Err(problem) = outcome {
            self.push(problem);
        }
    }

    /// Whether no problems have been recorded.
    pub fn is_empty(&self) -> bool {
        return self.problems.is_empty();
    }

    /// How many problems have been recorded.
    pub fn len(&self) -> usize {
        return self.problems.len();
    }

    /// Convert the collected problems into a result.
    ///
    /// No problems give `Ok(())`. One problem returns as itself, so the caller
    /// sees the error that a stop-at-first check gives. Two or more problems go
    /// into an [`Error::Validation`] in discovery order.
    pub fn into_result(mut self) -> Result<()> {
        // `pop` and not an index, because the workspace denies
        // `indexing_slicing`. This path also makes no new allocation.
        if self.problems.len() == 1 {
            match self.problems.pop() {
                Some(single) => return Err(single),
                None => return Ok(()),
            }
        }
        if self.problems.is_empty() {
            return Ok(());
        }
        return Err(Error::Validation {
            problems: self.problems,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(name: &str) -> Error {
        return Error::TypeNameCollision {
            name: name.to_owned(),
            artifact: "response enum".to_owned(),
            hint: "rename it".to_owned(),
        };
    }

    #[test]
    fn empty_collector_is_ok() {
        let diagnostics = Diagnostics::new();
        assert!(diagnostics.is_empty());
        assert!(diagnostics.into_result().is_ok());
    }

    #[test]
    fn single_problem_is_reported_as_itself() {
        // One problem must stay unwrapped. A caller matches one variant, and the
        // message must carry no count.
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(problem("Widget"));
        let err = diagnostics.into_result().expect_err("one problem must fail");
        assert!(
            matches!(&err, Error::TypeNameCollision { name, .. } if name == "Widget"),
            "expected the original variant unwrapped, got: {err:?}",
        );
    }

    #[test]
    fn multiple_problems_aggregate_in_order() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(problem("Widget"));
        diagnostics.check(Err(problem("Gadget")));
        diagnostics.check(Ok(()));
        assert_eq!(diagnostics.len(), 2, "a passing check must not be recorded");
        let err = diagnostics.into_result().expect_err("two problems must fail");
        let Error::Validation { problems } = &err else {
            panic!("expected Validation, got: {err:?}");
        };
        assert_eq!(problems.len(), 2);
        let message = err.to_string();
        let widget = message.find("Widget").expect("message should list the first problem");
        let gadget = message.find("Gadget").expect("message should list the second problem");
        assert!(
            widget < gadget,
            "problems should be listed in discovery order: {message}"
        );
        assert!(
            message.contains('2'),
            "message should say how many problems were found: {message}",
        );
    }

    #[test]
    fn a_report_pushed_into_a_report_does_not_nest() {
        // A schema-level check collects on its own and gives a report back. The
        // loop over schemas collects again. Without opening the inner report the
        // reader sees "found 2 problems", and one of the two hides the rest.
        let mut inner = Diagnostics::new();
        inner.push(problem("Widget"));
        inner.push(problem("Gadget"));
        let report = inner.into_result().expect_err("two problems must fail");

        let mut outer = Diagnostics::new();
        outer.push(report);
        outer.push(problem("Doohickey"));
        assert_eq!(outer.len(), 3, "the inner problems should be counted one by one");

        let err = outer.into_result().expect_err("three problems must fail");
        let Error::Validation { problems } = &err else {
            panic!("expected Validation, got: {err:?}");
        };
        assert!(
            problems
                .iter()
                .all(|entry| return !matches!(*entry, Error::Validation { .. })),
            "a report must hold leaf problems only, got: {problems:?}",
        );
    }
}
