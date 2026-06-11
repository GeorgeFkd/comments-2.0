use crate::models::{CommentData, HashCheckResult, StampParseError};
use std::ops::Fn;

#[derive(Debug)]
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
        CommentIntegrityRuleViolations::DependsOnCommentThatDoesntExist => (
            ViolationLevel::Error,
            "Comment depends on comment that has been deleted".to_string(),
        ),
        CommentIntegrityRuleViolations::DependsOnCommentThatChanged => (
            ViolationLevel::Error,
            "Comment depends on comment that has changed".to_string(),
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
            "::{} file={},line={},col={}::{} deps: {:?} id: {}",
            level,
            comment.file.display(),
            comment.comment_location.start.row,
            comment.comment_location.start.column,
            msg,
            comment.dependency_list_parsed,
            comment.id
        ),
        "editor" => format!(
            "{}:{}:{}: {}: {} deps: {:?} id: {}",
            comment.file.display(),
            comment.comment_location.start.row,
            //the cursor is at a slightly wrong place this is a temp fix ```comments-2.0 1 4395583177411532991 4395583177411532991 13```
            comment.comment_location.start.column + 1,
            level,
            msg,
            comment.dependency_list_parsed,
            comment.id
        ),
        //TODO: csv and db output
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

#[derive(PartialEq, Eq, Debug)]
pub enum CommentIntegrityRuleViolations {
    CommentDoesNotReferenceSpecificCode,
    ParseErrorThatShouldBeFixed,
    CodeChangedCommentNot,
    CommentHashNotRegenerated,
    CommentThatOthersDependOnChanged,
    CommentThatOthersDependOnDeleted,
    DependsOnCommentThatDoesntExist,
    DependsOnCommentThatChanged,
}

//this function will be configurable to return success/failure based on user input ```comments-2.0 16 14130792760320861292 14130792760320861292 14```
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

    let hash_check_result = check_that_stamp_is_updated(comment);
    println!("Hash check result is: {:?}", hash_check_result);
    match hash_check_result {
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

fn get_dependecy_violations<'a>(
    comments_of_project: &'a Vec<CommentData<'a>>,
) -> Vec<RuleViolationOnFile<'a>> {
    let comments_with_dependencies = comments_of_project
        .iter()
        .filter(|cm| cm.dependency_list_parsed.len() > 0);
    let mut result = vec![];
    for cm_with_deps in comments_with_dependencies {
        for dep in cm_with_deps.dependency_list_parsed.iter() {
            let found = comments_of_project
                .iter()
                .find(|cm| cm.id.to_owned().eq(&dep.0));
            if found.is_none() {
                println!(
                    "Found a dependency violation for comment: {:?}",
                    cm_with_deps.raw_contents
                );

                result.push(RuleViolationOnFile {
                    comment: cm_with_deps,
                    violation: CommentIntegrityRuleViolations::DependsOnCommentThatDoesntExist,
                });
            } else {
                if dep.1 != found.unwrap().comment_hash_parsed {
                    result.push(RuleViolationOnFile {
                        comment: cm_with_deps,
                        violation: CommentIntegrityRuleViolations::DependsOnCommentThatChanged,
                    })
                }
                //TODO: if a comment depends on a comment that its code changed it should be another
                //comment integrity violation
            }
        }
    }
    return result;
}

pub fn generate_violations_from_comments<'a>(
    comments_of_project: &'a Vec<CommentData<'a>>,
) -> Vec<RuleViolationOnFile<'a>> {
    let mut violations_within_comment: Vec<RuleViolationOnFile<'_>> = comments_of_project
        .iter()
        .filter_map(get_violation)
        .collect();
    let violations_between_comments = get_dependecy_violations(comments_of_project);
    violations_within_comment.extend(violations_between_comments);
    return violations_within_comment;
}

#[cfg(test)]
mod tests {

    // (not yet implemented) CommentThatOthersDependOnDeleted,
    mod helpers {
        use crate::models::{self, CommentData};
        pub fn update_hashes_like_parsed(mut cm: CommentData) -> CommentData {
            cm.code_hash_parsed = cm.hash_code();
            cm.comment_hash_parsed = cm.hash_comment();
            return cm;
        }

        pub fn normal_comment_with<'a>(comment: &str, code: &str) -> CommentData<'a> {
            let mut cm = CommentData::empty();
            let _ = cm.push_comment(comment);
            let _ = cm.push_code(code);
            let mut cm = update_hashes_like_parsed(cm);
            cm.parse_error = None;
            return cm;
        }
    }

    mod comment_should_generate_violation {
        use crate::{
            models::CommentData,
            violations::{
                CommentIntegrityRuleViolations, generate_violations_from_comments, get_violation,
                tests::helpers::{normal_comment_with, update_hashes_like_parsed},
            },
        };
        use std::path::Path;

        #[test]
        fn if_it_does_not_reference_specific_code() {
            let mut cm = CommentData::empty();
            cm.file = Path::new("hello.js");
            let _ = cm.push_comment("//Hello World");
            let _ = cm.push_code("console.log(`Hello World`)");
            let result = get_violation(&cm)
                .expect("Should produce violation when not referencing specific code");
            let expected = CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode;

            assert_eq!(result.violation, expected);
        }

        #[test]
        fn if_changed_code_is_not_regenerated() {
            let mut cm = CommentData::empty();
            let _ = cm.push_comment("//Hello World ```comments-2.0 1```");
            let _ = cm.push_code("console.log(`Hello World`)");
            let mut cm = update_hashes_like_parsed(cm);
            let _ = cm.push_code("console.log(`Hello World 2`)");
            //We need to set it to none, as we dont have the hashes yet and the parser correctly
            //detects a parse error which is later used in the calculation of the violation
            cm.parse_error = None;
            let result = get_violation(&cm).expect(
                "Should produce violation when code is changed without the comment being changed",
            );
            let expected = CommentIntegrityRuleViolations::CodeChangedCommentNot;

            assert_eq!(result.violation, expected);
        }

        #[test]
        fn if_changed_comment_is_not_regenerated() {
            let mut cm = CommentData::empty();
            let _ = cm.push_comment("//Hello World ```comments-2.0 1```");
            let _ = cm.push_code("console.log(`Hello World`)");
            let mut cm = update_hashes_like_parsed(cm);
            let _ = cm.push_comment("//Hello world some more comment");
            //We need to set it to none, as we dont have the hashes yet and the parser correctly
            //detects a parse error which is later used in the calculation of the violation
            cm.parse_error = None;
            let result = get_violation(&cm).expect(
                "Should produce violation when code is changed without the comment being changed",
            );
            let expected = CommentIntegrityRuleViolations::CommentHashNotRegenerated;

            assert_eq!(result.violation, expected);
        }

        #[test]
        fn if_comment_dependency_changes_comment() {
            let mut cm_to_depend_on = normal_comment_with(
                "//Hello World ```comments-2.0 1```",
                "console.log(`Hello World`)",
            );
            let id_of_comment_depended_on = 14;
            cm_to_depend_on.id = id_of_comment_depended_on;
            let cm_previous_hash = cm_to_depend_on.comment_hash_parsed.clone();
            let _ = cm_to_depend_on.push_comment("the edge case of 0 should be handled by caller");
            let cm_to_depend_on = update_hashes_like_parsed(cm_to_depend_on);

            let comment_text = format!(
                "//depends on Hello World impl ```comments-2.0 1 {}-{}```",
                id_of_comment_depended_on, cm_previous_hash
            );
            let dependant_cm = normal_comment_with(&comment_text, "console.log(hello_world())");

            let input = vec![cm_to_depend_on, dependant_cm];
            let result = generate_violations_from_comments(&input);
            println!("{:?}", result);
            assert_eq!(result.len(), 1);
            assert_eq!(
                result[0].violation,
                CommentIntegrityRuleViolations::DependsOnCommentThatChanged
            );
        }

        //there is a slight difference between the comment that doesnt exist
        //and the comment that was deleted, but to detect deletion we need a
        //previous snapshot somehow and that is not implemented yet(and therefore a separate test)
        //```comments-2.0 2```
        #[test]
        fn if_comment_dependency_is_nonexistent() {
            let mut cm_to_depend_on_but_not_added_in_check = normal_comment_with(
                "//Hello World ```comments-2.0 1```",
                "console.log(`Hello World`)",
            );
            let id_of_comment_depended_on = 14;
            cm_to_depend_on_but_not_added_in_check.id = id_of_comment_depended_on;
            let cm_hash = cm_to_depend_on_but_not_added_in_check
                .comment_hash_parsed
                .clone();
            let comment_text = format!(
                "//depends on Hello World impl ```comments-2.0 1 {}-{}```",
                id_of_comment_depended_on, cm_hash
            );
            let dependant_cm = normal_comment_with(&comment_text, "console.log(hello_world())");

            let input = vec![dependant_cm];
            let result = generate_violations_from_comments(&input);
            assert_eq!(result.len(), 1);
            assert_eq!(
                result[0].violation,
                CommentIntegrityRuleViolations::DependsOnCommentThatDoesntExist
            );
        }
    }

    mod comment_should_not_generate_violation {
        use crate::{
            models::CommentData,
            violations::{
                generate_violations_from_comments, get_violation,
                tests::helpers::{normal_comment_with, update_hashes_like_parsed},
            },
        };

        #[test]
        fn if_it_depends_on_up_to_date_comments() {
            let mut cm_to_depend_on = normal_comment_with(
                "//Hello World ```comments-2.0 1```",
                "console.log(`Hello World`)",
            );
            let id_of_comment_depended_on = 14;
            cm_to_depend_on.id = id_of_comment_depended_on;
            let cm_hash = cm_to_depend_on.comment_hash_parsed.clone();
            let comment_text = format!(
                "//depends on Hello World impl ```comments-2.0 1 {}-{}```",
                id_of_comment_depended_on, cm_hash
            );
            let dependant_cm = normal_comment_with(&comment_text, "console.log(hello_world())");

            let input = vec![cm_to_depend_on, dependant_cm];
            let result = generate_violations_from_comments(&input);
            assert_eq!(result.len(), 0);
        }

        //this test is basically the happy path```comments-2.0 2```
        #[test]
        fn if_it_has_not_changed_code_or_comment() {
            let mut cm = CommentData::empty();
            let _ = cm.push_comment("//Hello World ```comments-2.0 1```");
            let _ = cm.push_code("console.log(`Hello World`)");
            let mut cm = update_hashes_like_parsed(cm);
            cm.parse_error = None;

            let result = get_violation(&cm);

            assert!(result.is_none());
        }

        #[test]
        fn if_the_user_wants_to_ignore_it() {
            let mut cm = CommentData::empty();
            let _ = cm.push_comment("//Hello World ```comments-2.0 1```");
            let _ = cm.push_code("console.log(`Hello World`)");
            let mut cm = update_hashes_like_parsed(cm);
            let _ = cm.push_code("console.log(`Hello World 2`)");
            cm.parse_error = None;

            cm.should_be_ignored = true;

            let result = get_violation(&cm);

            assert!(result.is_none());
        }
    }

    mod should_respect_user_config {
        //it is basically the comment_should_generate_violation tests but with different configs, i
        //will see how to test it properly this one
    }
}
