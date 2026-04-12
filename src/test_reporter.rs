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

use std::io::Write;

/// Result of a single test execution.
pub struct TestResult {
    pub name: String,
    pub outcome: TestOutcome,
}

/// Whether a test passed, failed, or was skipped.
pub enum TestOutcome {
    Pass,
    Fail { message: String },
    Skip,
}

/// Trait for pluggable test output formats.
pub trait TestReporter {
    fn on_test_start(&mut self, name: &str);
    fn on_test_complete(&mut self, result: &TestResult);
    fn on_suite_complete(&mut self, results: &[TestResult]);
}

/// Basic text reporter that writes human-readable output.
pub struct TextReporter<W: Write> {
    writer: W,
}

impl<W: Write> TextReporter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> TestReporter for TextReporter<W> {
    fn on_test_start(&mut self, _name: &str) {
        // TextReporter prints results on completion, not on start
    }

    fn on_test_complete(&mut self, result: &TestResult) {
        match &result.outcome {
            TestOutcome::Pass => {
                writeln!(self.writer, "PASS  {}", result.name).ok();
            }
            TestOutcome::Fail { message } => {
                writeln!(self.writer, "FAIL  {}", result.name).ok();
                writeln!(self.writer, "      {}", message).ok();
            }
            TestOutcome::Skip => {
                writeln!(self.writer, "SKIP  {}", result.name).ok();
            }
        }
    }

    fn on_suite_complete(&mut self, results: &[TestResult]) {
        let total = results.len();
        let passed = results
            .iter()
            .filter(|r| matches!(r.outcome, TestOutcome::Pass))
            .count();
        let skipped = results
            .iter()
            .filter(|r| matches!(r.outcome, TestOutcome::Skip))
            .count();
        let failed = total - passed - skipped;

        writeln!(self.writer).ok();
        if skipped > 0 {
            writeln!(
                self.writer,
                "{} tests: {} passed, {} failed, {} skipped",
                total, passed, failed, skipped
            )
            .ok();
        } else {
            writeln!(
                self.writer,
                "{} tests: {} passed, {} failed",
                total, passed, failed
            )
            .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_reporter_all_pass() {
        let mut output = Vec::new();
        let mut reporter = TextReporter::new(&mut output);

        let results = vec![
            TestResult {
                name: "testA".to_string(),
                outcome: TestOutcome::Pass,
            },
            TestResult {
                name: "testB".to_string(),
                outcome: TestOutcome::Pass,
            },
        ];

        for r in &results {
            reporter.on_test_complete(r);
        }
        reporter.on_suite_complete(&results);

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("PASS  testA"));
        assert!(output_str.contains("PASS  testB"));
        assert!(output_str.contains("2 tests: 2 passed, 0 failed"));
    }

    #[test]
    fn test_text_reporter_with_failure_and_skip() {
        let mut output = Vec::new();
        let mut reporter = TextReporter::new(&mut output);

        let results = vec![
            TestResult {
                name: "testGood".to_string(),
                outcome: TestOutcome::Pass,
            },
            TestResult {
                name: "testBad".to_string(),
                outcome: TestOutcome::Fail {
                    message: "expected 3, got 4".to_string(),
                },
            },
            TestResult {
                name: "testSkippy".to_string(),
                outcome: TestOutcome::Skip,
            },
        ];

        for r in &results {
            reporter.on_test_complete(r);
        }
        reporter.on_suite_complete(&results);

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("PASS  testGood"));
        assert!(output_str.contains("FAIL  testBad"));
        assert!(output_str.contains("expected 3, got 4"));
        assert!(output_str.contains("SKIP  testSkippy"));
        assert!(output_str.contains("3 tests: 1 passed, 1 failed, 1 skipped"));
    }

    #[test]
    fn test_text_reporter_empty_suite() {
        let mut output = Vec::new();
        let mut reporter = TextReporter::new(&mut output);

        let results: Vec<TestResult> = vec![];
        reporter.on_suite_complete(&results);

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("0 tests: 0 passed, 0 failed"));
    }
}
