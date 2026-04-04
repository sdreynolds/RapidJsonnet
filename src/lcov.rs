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

        // Collect hit lines from all recorded spans
        let mut hit_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for &(start, _end) in spans {
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
}
