use crate::models::{CommentData, HashCheckResult, StampParseError};
use std::ops::Fn;
pub struct RuleViolationOnFile<'a> {
    pub violation: CommentIntegrityRuleViolations,
    pub comment: &'a CommentData<'a>,
}
#[derive(PartialEq, Eq)]
pub enum ViolationLevel {
    Warning,
    Error,
    Note,
}

impl ViolationLevel {
    pub fn as_str(&self) -> &str {
        match self {
            ViolationLevel::Warning => "warning",
            ViolationLevel::Error => "error",
            ViolationLevel::Note => "notice",
        }
    }
}

fn violation_to_message<'a>(
    violation: &'a CommentIntegrityRuleViolations,
) -> (ViolationLevel, String) {
    match violation {
        CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode => (
            ViolationLevel::Warning,
            "Comment does not reference specific code (no stamp found)".to_string(),
        ),
        CommentIntegrityRuleViolations::ParseErrorThatShouldBeFixed => {
            (ViolationLevel::Error, "Stamp has a parse error".to_string())
        }
        CommentIntegrityRuleViolations::CodeChangedCommentNot => (
            ViolationLevel::Error,
            format!("Code hash changed but comment was not updated",),
        ),
        CommentIntegrityRuleViolations::CommentHashNotRegenerated => (
            ViolationLevel::Warning,
            format!("Comment hash needs regeneration",),
        ),
        CommentIntegrityRuleViolations::CommentThatOthersDependOnChanged => (
            ViolationLevel::Error,
            "Comment changed but other comments depend on it".to_string(),
        ),
        CommentIntegrityRuleViolations::CommentThatOthersDependOnDeleted => (
            ViolationLevel::Error,
            "Comment deleted but other comments depend on it".to_string(),
        ),
    }
}

pub fn check_that_stamp_is_updated(comment: &CommentData) -> HashCheckResult {
    if comment.parse_error.is_some()
        && comment.parse_error.clone().unwrap() == StampParseError::StampWithoutHashes
    {
        return HashCheckResult::HashesShouldBeGenerated;
    }
    let code_hash_is_updated = comment.hash_code() == comment.code_hash_parsed;
    let comment_hash_is_updated = comment.hash_comment() == comment.comment_hash_parsed;
    if code_hash_is_updated && comment_hash_is_updated {
        return HashCheckResult::BothHashesUpToDate;
    }

    if code_hash_is_updated && !comment_hash_is_updated {
        return HashCheckResult::CommentHashNotUpToDate;
    }

    if !code_hash_is_updated && comment_hash_is_updated {
        return HashCheckResult::CodeHashNotUpToDate;
    }
    return HashCheckResult::BothHashesInvalid;
}

impl<'a> RuleViolationOnFile<'a> {
    pub fn display_to_user(&self, output_format: &str) -> (String, String) {
        let (level, message) = violation_to_message(&self.violation);
        (
            format_violation(output_format, level.as_str(), &self.comment, &message),
            level.as_str().to_owned(),
        )
    }
}

fn format_violation(output_format: &str, level: &str, comment: &CommentData, msg: &str) -> String {
    match output_format {
        "github" => format!(
            "::{} file={},line={},col={}::{}",
            level,
            comment.file.display(),
            comment.comment_location.start.row,
            comment.comment_location.start.column,
            msg
        ),
        "editor" => format!(
            "{}:{}:{}: {}: {}",
            comment.file.display(),
            comment.comment_location.start.row,
            //the cursor is at a slightly wrong place this is a temp fix ```comments-2.0 1```
            comment.comment_location.start.column + 1,
            level,
            msg
        ),
        _ => format!("Not a valid output format {output_format}"),
    }
}

pub fn display_violations_to_user(
    violations: &[RuleViolationOnFile],
    output_format: &str,
) -> String {
    if violations.is_empty() {
        return "No violations found! All comments are up to date.".to_string();
    }

    let mut result = format!("Found {} violation(s):\n\n", violations.len());

    for (idx, violation) in violations.iter().enumerate() {
        result.push_str(&violation.display_to_user(output_format).0);
        if idx < violations.len() - 1 {
            result.push_str("\n\n");
        }
    }

    result
}

pub enum CommentIntegrityRuleViolations {
    CommentDoesNotReferenceSpecificCode,
    ParseErrorThatShouldBeFixed,
    CodeChangedCommentNot,
    CommentHashNotRegenerated,
    CommentThatOthersDependOnChanged,
    CommentThatOthersDependOnDeleted,
}

//this function will be configurable to return success/failure based on user input ```comments-2.0 16```
pub fn determine_exit_code(violations: &[RuleViolationOnFile]) -> std::process::ExitCode {
    if violations.is_empty() {
        return std::process::ExitCode::SUCCESS;
    }

    let has_errors = violations
        .iter()
        .map(|rv| violation_to_message(&rv.violation))
        .any(|(level, _)| level == ViolationLevel::Error);

    if has_errors {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
fn get_violation<'a>(comment: &'a CommentData<'a>) -> Option<RuleViolationOnFile<'a>> {
    if comment.should_be_ignored {
        return None;
    }

    // Check for parse errors first
    if let Some(ref parse_err) = comment.parse_error {
        match parse_err {
            StampParseError::NoStampFound => {
                return Some(RuleViolationOnFile {
                    violation: CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode,
                    comment,
                });
            }
            StampParseError::StampWithoutClosingTag
            | StampParseError::StampWithoutLinesReferenced
            | StampParseError::StampWithoutCodeHash => {
                return Some(RuleViolationOnFile {
                    violation: CommentIntegrityRuleViolations::ParseErrorThatShouldBeFixed,
                    comment,
                });
            }
            StampParseError::StampWithoutHashes => {
                // This is handled separately below in hash check
            }
        }
    }

    match check_that_stamp_is_updated(comment) {
        HashCheckResult::HashesShouldBeGenerated => Some(RuleViolationOnFile {
            violation: CommentIntegrityRuleViolations::CommentHashNotRegenerated,
            comment,
        }),
        HashCheckResult::CodeHashNotUpToDate | HashCheckResult::BothHashesInvalid => {
            Some(RuleViolationOnFile {
                violation: CommentIntegrityRuleViolations::CodeChangedCommentNot,
                comment,
            })
        }
        HashCheckResult::CommentHashNotUpToDate => Some(RuleViolationOnFile {
            violation: CommentIntegrityRuleViolations::CommentHashNotRegenerated,
            comment,
        }),
        HashCheckResult::BothHashesUpToDate => None,
    }
}
pub fn generate_violations_from_comments<'a>(
    comments_of_project: &'a Vec<CommentData<'a>>,
) -> Vec<RuleViolationOnFile<'a>> {
    comments_of_project
        .iter()
        .filter_map(get_violation)
        .collect()
}
