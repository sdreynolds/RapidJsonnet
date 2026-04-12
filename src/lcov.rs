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

use coverage::CoverageCollector;

/// Generate LCOV-formatted coverage report from a CoverageCollector.
/// For each source file, reads it from disk, maps byte-offset spans to line numbers,
/// and emits DA records for every line in the file (hit=1 or hit=0).
/// `test_name` is written to the TN: field in each LCOV record.
pub fn generate_lcov(collector: &CoverageCollector, test_name: &str) -> String {
    let mut output = String::new();
    let mut source_ids: Vec<&str> = collector.source_ids();
    source_ids.sort(); // deterministic output order

    for source_id in source_ids {
        let spans = match collector.spans_for_source(source_id) {
            Some(s) => s,
            None => continue,
        };

        let content = match std::fs::read(source_id) {
            Ok(b) => b,
            Err(_) => {
                // Gracefully skip files we cannot read
                output.push_str(&format!("# Could not read source: {}\n", source_id));
                continue;
            }
        };

        // Build byte_offset -> line_number mapping (1-indexed).
        // Also classify each line: lines that contain only whitespace or only
        // structural bracket characters ({, }, [, ]) are not executable and
        // should not get DA records.
        let mut byte_to_line: Vec<usize> = Vec::with_capacity(content.len() + 1);
        let mut line_has_content: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        // Track lines that have *only* structural/bracket bytes (no other content).
        // A line is bracket-only if every non-whitespace byte is one of { } [ ].
        let mut line_non_bracket_content: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        let mut current_line = 1usize;
        for &byte in &content {
            byte_to_line.push(current_line);
            if byte == b'\n' {
                current_line += 1;
            } else if !matches!(byte, b' ' | b'\t' | b'\r') {
                line_has_content.insert(current_line);
                if !matches!(byte, b'{' | b'}' | b'[' | b']') {
                    line_non_bracket_content.insert(current_line);
                }
            }
        }
        byte_to_line.push(current_line); // sentinel for spans ending at EOF

        // A trailing newline increments current_line past the last real line.
        // Subtract one in that case so we don't emit a phantom DA record.
        let total_lines = if content.last() == Some(&b'\n') {
            current_line - 1
        } else {
            current_line
        };

        // Collect hit lines from all recorded spans.
        // Use the full byte range [start, end) so that multi-line expressions
        // (e.g. a function call whose arguments span several lines) mark every
        // covered line, not just the line where the expression begins.
        let mut hit_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for &(start, end) in spans {
            let clamped_end = end.min(byte_to_line.len());
            for byte_idx in start..clamped_end {
                hit_lines.insert(byte_to_line[byte_idx]);
            }
            // Also mark the start line even if end <= start (zero-length span)
            if start < byte_to_line.len() {
                hit_lines.insert(byte_to_line[start]);
            }
        }

        // Emit LCOV record for this source file
        output.push_str(&format!("TN:{}\n", test_name));
        output.push_str(&format!("SF:{}\n", source_id));

        let mut lines_found = 0usize;
        let mut lines_hit = 0usize;
        for line_num in 1..=total_lines {
            // Skip empty, whitespace-only, and bracket-only lines — not executable
            if !line_has_content.contains(&line_num)
                || !line_non_bracket_content.contains(&line_num)
            {
                continue;
            }
            let hit_count = if hit_lines.contains(&line_num) { 1 } else { 0 };
            output.push_str(&format!("DA:{},{}\n", line_num, hit_count));
            lines_found += 1;
            if hit_count > 0 {
                lines_hit += 1;
            }
        }

        output.push_str(&format!("LH:{}\n", lines_hit));
        output.push_str(&format!("LF:{}\n", lines_found));
        output.push_str("end_of_record\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use coverage::CoverageCollector;
    use std::io::Write;

    /// Write `content` to a temp file and return its path as a String.
    fn write_temp(name: &str, content: &[u8]) -> String {
        let path = std::env::temp_dir().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// Parse the DA records out of an LCOV string for a single-file report.
    /// Returns a Vec of (line_number, hit_count).
    fn parse_da_records(lcov: &str) -> Vec<(usize, usize)> {
        lcov.lines()
            .filter(|l| l.starts_with("DA:"))
            .map(|l| {
                let rest = &l[3..];
                let mut parts = rest.splitn(2, ',');
                let line: usize = parts.next().unwrap().parse().unwrap();
                let hit: usize = parts.next().unwrap().parse().unwrap();
                (line, hit)
            })
            .collect()
    }

    #[test]
    fn test_generate_lcov_empty_collector() {
        let collector = CoverageCollector::new();
        let result = generate_lcov(&collector, "");
        assert!(result.is_empty());
    }

    #[test]
    fn test_generate_lcov_missing_file() {
        let mut collector = CoverageCollector::new();
        collector.record("/nonexistent/file.jsonnet", &(0..5));
        let result = generate_lcov(&collector, "my_test");
        assert!(result.contains("# Could not read source:"));
        assert!(!result.contains("SF:"));
    }

    #[test]
    fn test_empty_lines_excluded() {
        // Line 1: "x" (content), line 2: "" (empty), line 3: "y" (content)
        let path = write_temp("lcov_test_empty_lines.jsonnet", b"x\n\ny\n");
        let mut collector = CoverageCollector::new();
        collector.record(&path, &(0..1)); // hit "x" on line 1
        let result = generate_lcov(&collector, "");
        let da = parse_da_records(&result);
        // Line 2 (empty) must not appear at all
        assert!(
            !da.iter().any(|(ln, _)| *ln == 2),
            "empty line 2 should be excluded: {:?}",
            da
        );
        assert!(da.contains(&(1, 1)), "line 1 should be hit");
        assert!(da.contains(&(3, 0)), "line 3 should be present but unhit");
    }

    #[test]
    fn test_whitespace_only_lines_excluded() {
        // Line 1: "a" (content), line 2: "   " (spaces only), line 3: "b"
        let path = write_temp("lcov_test_whitespace.jsonnet", b"a\n   \nb\n");
        let mut collector = CoverageCollector::new();
        collector.record(&path, &(0..1));
        let result = generate_lcov(&collector, "");
        let da = parse_da_records(&result);
        assert!(
            !da.iter().any(|(ln, _)| *ln == 2),
            "whitespace-only line 2 should be excluded: {:?}",
            da
        );
    }

    #[test]
    fn test_bracket_only_lines_excluded() {
        // Each line has only bracket chars (possibly mixed)
        let content = b"{\n}\n[\n]\na\n";
        let path = write_temp("lcov_test_brackets.jsonnet", content);
        let mut collector = CoverageCollector::new();
        // Hit "a" on line 5 (byte offset 8)
        collector.record(&path, &(8..9));
        let result = generate_lcov(&collector, "");
        let da = parse_da_records(&result);
        // Lines 1-4 are all bracket-only — must not appear
        for ln in 1..=4usize {
            assert!(
                !da.iter().any(|(l, _)| *l == ln),
                "bracket-only line {} should be excluded: {:?}",
                ln,
                da
            );
        }
        assert!(da.contains(&(5, 1)), "line 5 should be hit");
    }

    #[test]
    fn test_mixed_bracket_and_content_line_included() {
        // A line like "{ key:" has a bracket AND other content — must be included
        let content = b"{ key: 1 }\n";
        let path = write_temp("lcov_test_mixed_bracket.jsonnet", content);
        let mut collector = CoverageCollector::new();
        collector.record(&path, &(0..1));
        let result = generate_lcov(&collector, "");
        let da = parse_da_records(&result);
        assert!(
            da.contains(&(1, 1)),
            "mixed-content line should be included and hit: {:?}",
            da
        );
    }

    #[test]
    fn test_trailing_newline_no_phantom_line() {
        // File ends with \n — must not emit a DA record for a phantom line beyond the last real one
        let content = b"x\ny\n"; // 2 real lines
        let path = write_temp("lcov_test_trailing_newline.jsonnet", content);
        let mut collector = CoverageCollector::new();
        collector.record(&path, &(0..1));
        let result = generate_lcov(&collector, "");
        let da = parse_da_records(&result);
        let max_line = da.iter().map(|(ln, _)| *ln).max().unwrap_or(0);
        assert_eq!(
            max_line, 2,
            "last DA line should be 2, not a phantom line 3: {:?}",
            da
        );
        let lf: usize = result.lines().find(|l| l.starts_with("LF:")).unwrap()[3..]
            .parse()
            .unwrap();
        assert_eq!(lf, 2, "LF should count 2 lines, not 3: {}", result);
    }

    #[test]
    fn test_no_trailing_newline() {
        // File does NOT end with \n — last line must still appear
        let content = b"x\ny"; // 2 real lines, no trailing newline
        let path = write_temp("lcov_test_no_trailing_newline.jsonnet", content);
        let mut collector = CoverageCollector::new();
        collector.record(&path, &(2..3)); // hit "y" on line 2
        let result = generate_lcov(&collector, "");
        let da = parse_da_records(&result);
        assert!(
            da.contains(&(2, 1)),
            "line 2 must appear when file has no trailing newline: {:?}",
            da
        );
    }

    #[test]
    fn test_multiline_span_marks_all_covered_lines() {
        // Simulates: someFunc(\n  arg\n) — a span that starts on line 1 and ends on line 3.
        // All three lines should be marked as hit.
        // content: "f(\n  a\n)\n"
        //   line 1: "f("   bytes 0-2
        //   line 2: "  a"  bytes 3-6
        //   line 3: ")"    bytes 7-8
        let content = b"f(\n  a\n)\n";
        let path = write_temp("lcov_test_multiline_span.jsonnet", content);
        let mut collector = CoverageCollector::new();
        // Span covers the entire expression: byte 0 ("f") to byte 8 (")")
        collector.record(&path, &(0..8));
        let result = generate_lcov(&collector, "");
        let da = parse_da_records(&result);
        assert!(da.contains(&(1, 1)), "line 1 should be hit: {:?}", da);
        assert!(da.contains(&(2, 1)), "line 2 should be hit: {:?}", da);
        assert!(da.contains(&(3, 1)), "line 3 should be hit: {:?}", da);
    }

    #[test]
    fn test_test_name_in_tn_field() {
        let path = write_temp("lcov_test_tn.jsonnet", b"x\n");
        let mut collector = CoverageCollector::new();
        collector.record(&path, &(0..1));
        let result = generate_lcov(&collector, "my_target");
        assert!(
            result.contains("TN:my_target\n"),
            "TN field should contain test name: {}",
            result
        );
    }
}
