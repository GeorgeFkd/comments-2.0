use std::io::BufRead;

use crate::models::{CommentData, StampParseError};
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub fn regenerate_hashes_in_files<'a>(
    comments_by_file: impl Iterator<Item = (&'a Path, Vec<&'a CommentData<'a>>)>,
) -> Result<(), String> {
    for (file_path, file_comments) in comments_by_file {
        let content_changes = collect_hash_insertions(&file_comments);

        if content_changes.is_empty() {
            println!("No changes to be made in file {}", file_path.display());
            continue;
        }

        apply_hash_insertions(file_path, content_changes)?;
    }

    println!("Hash generation complete.");
    Ok(())
}

fn collect_hash_insertions(comments: &[&CommentData]) -> Vec<(usize, usize, String)> {
    comments
        .iter()
        .filter(|c| !c.should_be_ignored)
        .filter_map(|comment| {
            if let Some(StampParseError::StampWithoutHashes) = comment.parse_error {
                if let Some(ref stamp_end) = comment.stamp_end {
                    return Some((
                        stamp_end.row - 1,
                        stamp_end.column,
                        format!(" {} {}", comment.hash_comment(), comment.hash_code()),
                    ));
                }
            }
            None
        })
        .collect()
}

fn apply_hash_insertions(
    file_path: &Path,
    content_changes: Vec<(usize, usize, String)>,
) -> Result<(), String> {
    println!(
        "Adding {} hash(es) to {}",
        content_changes.len(),
        file_path.display()
    );

    let reader = BufReader::new(
        File::open(file_path)
            .map_err(|e| format!("Failed to open {}: {}", file_path.display(), e))?,
    );

    let content_changes_refs: Vec<(usize, usize, &str)> = content_changes
        .iter()
        .map(|(row, col, s)| (*row, *col, s.as_str()))
        .collect();

    let modified_content = with_multiple_added_content_at(reader, content_changes_refs)?;

    fs::write(file_path, modified_content)
        .map_err(|e| format!("Failed to write {}: {}", file_path.display(), e))?;

    Ok(())
}

pub fn with_multiple_added_content_at<T: BufRead>(
    reader: T,
    content_changes: Vec<(usize, usize, &str)>,
) -> Result<String, String> {
    for (row, col, content) in &content_changes {
        if content.contains('\n') {
            return Err(format!(
                "Content at position ({}, {}) contains newline character, which is not allowed",
                row, col
            ));
        }
    }

    // Sort by row first, then by column(so we can apply changes sequentially and not have to
    // shift everything everytime we apply a change)
    let mut sorted_changes = content_changes;
    sorted_changes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // We assume that changes do not appear in the same row and return an error if that
    // happens.
    for i in 0..sorted_changes.len().saturating_sub(1) {
        if sorted_changes[i].0 == sorted_changes[i + 1].0 {
            return Err(format!(
                "Conflict: multiple changes on the same row {}",
                sorted_changes[i].0
            ));
        }
    }

    let mut result = String::new();
    let mut change_idx = 0;

    let lines: Vec<_> = reader
        .lines()
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Error reading lines: {}", e))?;

    for (row_num, line) in lines.iter().enumerate() {
        let current_row = row_num;

        // Check if we have a change for this row
        if change_idx < sorted_changes.len() && sorted_changes[change_idx].0 == current_row {
            let (_, col, content) = sorted_changes[change_idx];
            let col = col as usize;

            // Insert content at the specified column
            let mut modified_line = String::new();
            modified_line.push_str(&line[..col.min(line.len())]);
            modified_line.push_str(content);
            modified_line.push_str(&line[col.min(line.len())..]);

            result.push_str(&modified_line);
            change_idx += 1;
        } else {
            result.push_str(line);
        }

        // Add back the newline (lines() strips them)
        if row_num < lines.len() - 1 {
            result.push('\n');
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn content_with_newline_returns_error() {
        let input = "Hello world
This is a test";
        let reader = BufReader::new(input.as_bytes());

        let content_changes = vec![
            (0, 5, " GOOD"),
            (1, 0, "BAD\nCONTENT"), // Contains newline
        ];

        let result = with_multiple_added_content_at(reader, content_changes);

        assert!(
            result.is_err(),
            "Should return error for content with newline"
        );
        assert!(
            result.unwrap_err().contains("newline"),
            "Error message should mention newline"
        );
    }

    #[test]
    fn same_row_returns_error() {
        let input = "Hello world
This is a test";
        let reader = BufReader::new(input.as_bytes());

        let content_changes = vec![
            (0, 3, " FIRST"),
            (0, 5, " SECOND"), // Same row as previous
        ];

        let result = with_multiple_added_content_at(reader, content_changes);

        assert!(
            result.is_err(),
            "Should return error for duplicate positions"
        );
        assert!(
            result.unwrap_err().contains("Conflict"),
            "Error message should mention conflict"
        );
    }

    #[test]
    fn all_changes_appear_in_result() {
        let input = "Hello world
This is a test
Another line";
        let reader = BufReader::new(input.as_bytes());
        let content_changes = vec![
            (0, 5, " INSERTED"), // Insert after "Hello"
            (1, 0, "PREFIX "),   // Insert at beginning of line 2
            (2, 7, "XXX"),       // Insert in middle of line 3
        ];

        let result = with_multiple_added_content_at(reader, content_changes.clone())
            .expect("Function should succeed");

        let any_changes_already_in_old_string =
            content_changes.iter().any(|(_, _, s)| input.contains(s));
        let all_changes_appear_in_new_string =
            content_changes.iter().all(|(_, _, s)| result.contains(s));

        assert!(
            !any_changes_already_in_old_string && all_changes_appear_in_new_string,
            "Result should contain all of the changes added without containing them in the first place"
        );
    }

    #[test]
    fn result_size_equals_input_plus_changes() {
        let input = "Hello world
This is a test
Another line";
        let reader = BufReader::new(input.as_bytes());

        let content_changes = vec![
            (0, 5, " INSERTED"), // Insert after "Hello"
            (1, 0, "PREFIX "),   // Insert at beginning of line 2
            (2, 7, "XXX"),       // Insert in middle of line 3
        ];

        let input_size = input.len();
        let changes_size: usize = content_changes.iter().map(|(_, _, s)| s.len()).sum();
        let expected_size = input_size + changes_size;

        let result = with_multiple_added_content_at(reader, content_changes.clone())
            .expect("Function should succeed");
        println!("Input is: {}", input);
        println!("Result is: {}", result);
        assert_eq!(
            result.len(),
            expected_size,
            "Result size should be input size + sum of change lengths + newlines"
        );
    }
}
