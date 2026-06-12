use crate::models::{CommentData, SourceLocation};

use std::io::prelude::*;
use std::path::Path;

fn project_folder() -> String {
    "".into()
}

#[derive(Debug, PartialEq)]
enum State {
    Code,
    SingleLineComment,
    MultiLineComment,
    ReadingReferencedCode { remaining: usize },
}

pub fn parse_file<T: BufRead>(file: &Path, reader: T) -> Vec<CommentData<'_>> {
    let mut state = State::Code;
    let mut current_comment = CommentData::empty();
    current_comment.file = file;
    let mut result = Vec::new();
    let mut current_row = 0;

    for line in reader.lines().map_while(Result::ok) {
        current_row += 1;

        // Calculate column (where non-whitespace starts)
        let leading_whitespace = line.len() - line.trim_start().len();
        let current_column = leading_whitespace;
        let trimmed = line.trim_start();

        match state {
            State::Code => {
                if let Some(stripped) = trimmed.strip_prefix("/*") {
                    state = State::MultiLineComment;
                    current_comment.comment_location.start.row = current_row;
                    current_comment.comment_location.start.column = current_column;
                    let ind = current_comment.push_comment(stripped);
                    if let Some(pos_in_comment) = ind {
                        let pos_in_trimmed = "/*".len() + pos_in_comment;
                        let pos_in_original_line = current_column + pos_in_trimmed;
                        current_comment.stamp_end = Some(SourceLocation {
                            row: current_row,
                            column: pos_in_original_line,
                        });
                    }
                } else if let Some(stripped) = trimmed.strip_prefix("//") {
                    state = State::SingleLineComment;
                    current_comment.comment_location.start.row = current_row;
                    current_comment.comment_location.start.column = current_column;
                    let ind = current_comment.push_comment(stripped);
                    if let Some(pos_in_comment) = ind {
                        let pos_in_trimmed = "//".len() + pos_in_comment;
                        let pos_in_original_line = current_column + pos_in_trimmed;
                        current_comment.stamp_end = Some(SourceLocation {
                            row: current_row,
                            column: pos_in_original_line,
                        });
                    }
                }
            }

            State::SingleLineComment => {
                if let Some(stripped) = trimmed.strip_prefix("//") {
                    let ind = current_comment.push_comment(stripped);
                    if let Some(pos_in_comment) = ind {
                        let pos_in_trimmed = "//".len() + pos_in_comment;
                        let pos_in_original_line = current_column + pos_in_trimmed;
                        current_comment.stamp_end = Some(SourceLocation {
                            row: current_row,
                            column: pos_in_original_line,
                        });
                    }
                } else {
                    // Comment ended
                    current_comment.comment_location.end.row = current_row - 1;
                    let is_stamped = current_comment.lines_of_code_referenced > 0;

                    if !is_stamped {
                        result.push(current_comment);
                        current_comment = CommentData::empty();
                        current_comment.file = file;
                        state = State::Code;
                    } else {
                        current_comment.push_code(trimmed);
                        current_comment.lines_of_code_read = 1;

                        if current_comment.lines_of_code_read
                            == current_comment.lines_of_code_referenced
                        {
                            result.push(current_comment);
                            current_comment = CommentData::empty();
                            current_comment.file = file;
                            state = State::Code;
                        } else {
                            state = State::ReadingReferencedCode {
                                remaining: current_comment.lines_of_code_referenced as usize - 1,
                            };
                        }
                    }
                }
            }

            State::MultiLineComment => {
                if trimmed.starts_with("*/") {
                    current_comment.comment_location.end.row = current_row;
                    current_comment.comment_location.end.column = current_column;
                    let is_stamped = current_comment.lines_of_code_referenced > 0;

                    if is_stamped {
                        state = State::ReadingReferencedCode {
                            remaining: current_comment.lines_of_code_referenced as usize,
                        };
                    } else {
                        result.push(current_comment);
                        current_comment = CommentData::empty();
                        current_comment.file = file;
                        state = State::Code;
                    }
                } else {
                    let ind = current_comment.push_comment(trimmed);
                    if let Some(pos_in_comment) = ind {
                        let pos_in_original_line = current_column + pos_in_comment;
                        current_comment.stamp_end = Some(SourceLocation {
                            row: current_row,
                            column: pos_in_original_line,
                        });
                    }
                }
            }

            State::ReadingReferencedCode { remaining } => {
                current_comment.push_code(trimmed);
                current_comment.lines_of_code_read += 1;

                if remaining == 1 {
                    result.push(current_comment);
                    current_comment = CommentData::empty();
                    current_comment.file = file;
                    state = State::Code;
                } else {
                    state = State::ReadingReferencedCode {
                        remaining: remaining - 1,
                    };
                }
            }
        }
    }

    // Handle trailing comment
    if !current_comment.raw_contents.is_empty() {
        result.push(current_comment);
    }

    result
        .into_iter()
        .filter(|comment| !comment.raw_contents.is_empty())
        .collect()
}
