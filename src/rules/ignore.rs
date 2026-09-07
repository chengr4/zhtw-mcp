//! Per-project ignore terms.
//!
//! A term the user has declared uninteresting stays visible but stops
//! failing the build: severity drops to Info, which takes it out of both
//! the error and the warning gate.  Shared by the MCP `ignore_terms`
//! argument and the CLI `ignore_terms` config key so the two front ends
//! cannot disagree about what ignoring a term means.

use crate::rules::ruleset::{Issue, Severity};
use std::collections::HashSet;

/// Downgrade issues whose found term matches the ignore set to Info.
pub fn apply_ignore_set(issues: &mut [Issue], ignore_set: &HashSet<&str>) {
    if ignore_set.is_empty() {
        return;
    }
    for issue in issues {
        if ignore_set.contains(issue.found.as_str()) {
            issue.severity = Severity::Info;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/rules/ignore/tests.rs"]
mod tests;
