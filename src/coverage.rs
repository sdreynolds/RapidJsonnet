// Copyright 2026 Scott Reynolds
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Collects span-level coverage data during VM execution.
/// Keyed by source_id, values are sets of (start, end) span pairs.
pub struct CoverageCollector {
    hit_spans: HashMap<String, HashSet<(usize, usize)>>,
    /// (start, end, optional_name)
    hit_functions: HashMap<String, HashSet<(usize, usize, Option<String>)>>,
    /// (start, end, instruction_pointer, outcome_id)
    hit_branches: HashMap<String, HashSet<(usize, usize, usize, usize)>>,
}

impl Default for CoverageCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverageCollector {
    pub fn new() -> Self {
        Self {
            hit_spans: HashMap::new(),
            hit_functions: HashMap::new(),
            hit_branches: HashMap::new(),
        }
    }

    /// Record a span hit for a given source file.
    pub fn record(&mut self, source_id: &str, span: &Range<usize>) {
        self.hit_spans
            .entry(source_id.to_string())
            .or_default()
            .insert((span.start, span.end));
    }

    /// Record a function hit.
    pub fn record_function(&mut self, source_id: &str, span: &Range<usize>, name: Option<String>) {
        self.hit_functions
            .entry(source_id.to_string())
            .or_default()
            .insert((span.start, span.end, name));
    }

    /// Record a branch hit.
    pub fn record_branch(
        &mut self,
        source_id: &str,
        span: &Range<usize>,
        ip: usize,
        outcome: usize,
    ) {
        self.hit_branches
            .entry(source_id.to_string())
            .or_default()
            .insert((span.start, span.end, ip, outcome));
    }

    /// Merge another collector's data into this one.
    pub fn merge(&mut self, other: CoverageCollector) {
        for (source_id, spans) in other.hit_spans {
            self.hit_spans.entry(source_id).or_default().extend(spans);
        }
        for (source_id, funcs) in other.hit_functions {
            self.hit_functions
                .entry(source_id)
                .or_default()
                .extend(funcs);
        }
        for (source_id, branches) in other.hit_branches {
            self.hit_branches
                .entry(source_id)
                .or_default()
                .extend(branches);
        }
    }

    /// Get all source IDs that have coverage data.
    pub fn source_ids(&self) -> Vec<&str> {
        let mut ids: HashSet<&str> = self.hit_spans.keys().map(|s| s.as_str()).collect();
        ids.extend(self.hit_functions.keys().map(|s| s.as_str()));
        ids.extend(self.hit_branches.keys().map(|s| s.as_str()));
        ids.into_iter().collect()
    }

    /// Get the set of hit spans for a given source file.
    pub fn spans_for_source(&self, source_id: &str) -> Option<&HashSet<(usize, usize)>> {
        self.hit_spans.get(source_id)
    }

    /// Get the set of hit functions for a given source file.
    pub fn functions_for_source(
        &self,
        source_id: &str,
    ) -> Option<&HashSet<(usize, usize, Option<String>)>> {
        self.hit_functions.get(source_id)
    }

    /// Get the set of hit branches for a given source file.
    pub fn branches_for_source(
        &self,
        source_id: &str,
    ) -> Option<&HashSet<(usize, usize, usize, usize)>> {
        self.hit_branches.get(source_id)
    }

    /// Remove a source file from the coverage data.
    /// Used to exclude the test entrypoint file itself from coverage output.
    pub fn remove_source(&mut self, source_id: &str) {
        self.hit_spans.remove(source_id);
        self.hit_functions.remove(source_id);
        self.hit_branches.remove(source_id);
    }

    /// Total number of unique spans hit across all sources.
    pub fn total_spans_hit(&self) -> usize {
        self.hit_spans.values().map(|s| s.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_retrieve() {
        let mut collector = CoverageCollector::new();
        collector.record("test.jsonnet", &(0..5));
        collector.record("test.jsonnet", &(10..15));
        collector.record("other.jsonnet", &(0..3));

        assert_eq!(collector.source_ids().len(), 2);
        assert_eq!(collector.spans_for_source("test.jsonnet").unwrap().len(), 2);
        assert_eq!(
            collector.spans_for_source("other.jsonnet").unwrap().len(),
            1
        );
        assert_eq!(collector.total_spans_hit(), 3);
    }

    #[test]
    fn test_deduplication() {
        let mut collector = CoverageCollector::new();
        collector.record("test.jsonnet", &(0..5));
        collector.record("test.jsonnet", &(0..5));
        assert_eq!(collector.total_spans_hit(), 1);
    }

    #[test]
    fn test_merge() {
        let mut a = CoverageCollector::new();
        a.record("test.jsonnet", &(0..5));

        let mut b = CoverageCollector::new();
        b.record("test.jsonnet", &(10..15));
        b.record("other.jsonnet", &(0..3));

        a.merge(b);
        assert_eq!(a.source_ids().len(), 2);
        assert_eq!(a.spans_for_source("test.jsonnet").unwrap().len(), 2);
        assert_eq!(a.total_spans_hit(), 3);
    }

    #[test]
    fn test_empty_collector() {
        let collector = CoverageCollector::new();
        assert_eq!(collector.total_spans_hit(), 0);
        assert!(collector.spans_for_source("nope").is_none());
    }
}
