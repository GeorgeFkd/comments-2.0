use crate::models::{CommentData, HashCheckResult, StampParseError};
use std::{collections::HashMap, str::FromStr};

#[derive(Debug)]
pub struct RuleViolationOnFile<'a> {
    pub violation: CommentIntegrityRuleViolations,
    pub comment: &'a CommentData<'a>,
}
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
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

impl FromStr for ViolationLevel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "warning" => Ok(ViolationLevel::Warning),
            "error" => Ok(ViolationLevel::Error),
            "notice" => Ok(ViolationLevel::Note),
            other => Err(format!("unknown level: {other}")),
        }
    }
}

#[derive(Default)]
pub struct ViolationChecker {
    config: ViolationLevelsConfig,
}

pub struct ViolationLevelsConfig {
    path: String,
    mapping: HashMap<CommentIntegrityRuleViolations, ViolationLevel>,
}

impl Default for ViolationLevelsConfig {
    fn default() -> Self {
        use CommentIntegrityRuleViolations::*;
        use ViolationLevel::*;

        let mapping = HashMap::from([
            (CommentDoesNotReferenceSpecificCode, Warning),
            (ParseErrorThatShouldBeFixed, Error),
            (CodeChangedCommentNot, Error),
            (CommentHashNotRegenerated, Warning),
            (CommentThatOthersDependOnChanged, Error),
            (CommentThatOthersDependOnDeleted, Error),
            (DependsOnCommentThatDoesntExist, Error),
            (DependsOnCommentThatChanged, Error),
        ]);

        Self {
            path: String::new(),
            mapping,
        }
    }
}

impl ViolationLevelsConfig {
    pub fn level_of(&self, violation: CommentIntegrityRuleViolations) -> ViolationLevel {
        self.mapping
            .get(&violation)
            .copied()
            .unwrap_or(ViolationLevel::Note)
    }

    pub fn from_file(path: String) -> Result<Self, String> {
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mapping: HashMap<CommentIntegrityRuleViolations, ViolationLevel> = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim(), v.trim()))
            .map(|(k, v)| {
                Ok((
                    k.parse::<CommentIntegrityRuleViolations>()?,
                    v.parse::<ViolationLevel>()?,
                ))
            })
            .collect::<Result<HashMap<_, _>, String>>()?;
        Ok(Self { path, mapping })
    }
}

impl ViolationChecker {
    pub fn with_config_file(path: String) -> Result<Self, String> {
        let config = ViolationLevelsConfig::from_file(path)?;
        Ok(Self { config })
    }

    pub fn display_to_user(
        &self,
        file_violation: &RuleViolationOnFile,
        output_format: &str,
    ) -> (String, String) {
        let (level, message) = self.violation_to_message(&file_violation.violation);
        (
            format_violation(
                output_format,
                level.as_str(),
                file_violation.comment,
                &message,
            ),
            level.as_str().to_owned(),
        )
    }

    fn violation_to_message(
        &self,
        violation: &CommentIntegrityRuleViolations,
    ) -> (ViolationLevel, String) {
        let message = match violation {
            CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode => {
                "Comment does not reference specific code (no stamp found)".to_string()
            }
            CommentIntegrityRuleViolations::ParseErrorThatShouldBeFixed => {
                "Stamp has a parse error".to_string()
            }
            CommentIntegrityRuleViolations::CodeChangedCommentNot => {
                "Code hash changed but comment was not updated".to_string()
            }
            CommentIntegrityRuleViolations::CommentHashNotRegenerated => {
                "Comment hash needs regeneration".to_string()
            }
            CommentIntegrityRuleViolations::CommentThatOthersDependOnChanged => {
                "Comment changed but other comments depend on it".to_string()
            }
            CommentIntegrityRuleViolations::CommentThatOthersDependOnDeleted => {
                "Comment deleted but other comments depend on it".to_string()
            }
            CommentIntegrityRuleViolations::DependsOnCommentThatDoesntExist => {
                "Comment depends on comment that has been deleted".to_string()
            }
            CommentIntegrityRuleViolations::DependsOnCommentThatChanged => {
                "Comment depends on comment that has changed".to_string()
            }
        };

        let level = self.config.level_of(*violation);
        (level, message)
    }

    //not sure if this function needs to grab self
    pub fn get_violation<'a>(
        &self,
        comment: &'a CommentData<'a>,
    ) -> Option<RuleViolationOnFile<'a>> {
        if comment.should_be_ignored {
            return None;
        }

        // Check for parse errors first
        if let Some(ref parse_err) = comment.parse_error {
            match parse_err {
                StampParseError::NoStampFound => {
                    return Some(RuleViolationOnFile {
                        violation:
                            CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode,
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

        let hash_check_result = Self::check_that_stamp_is_updated(comment);
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

    fn check_that_stamp_is_updated(comment: &CommentData) -> HashCheckResult {
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
        HashCheckResult::BothHashesInvalid
    }

    pub fn determine_exit_code(
        &self,
        violations: &[RuleViolationOnFile],
    ) -> std::process::ExitCode {
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
    pub fn display_violations_to_user(
        &self,
        violations: &[RuleViolationOnFile],
        output_format: &str,
    ) -> String {
        if violations.is_empty() {
            return "No violations found! All comments are up to date.".to_string();
        }

        let mut result = format!("Found {} violation(s):\n\n", violations.len());

        for (idx, violation) in violations.iter().enumerate() {
            result.push_str(&self.display_to_user(violation, output_format).0);
            if idx < violations.len() - 1 {
                result.push_str("\n\n");
            }
        }

        result
    }

    pub fn generate_violations_from_comments<'a>(
        &self,
        comments_of_project: &'a Vec<CommentData<'a>>,
    ) -> Vec<RuleViolationOnFile<'a>> {
        let mut violations_within_comment: Vec<RuleViolationOnFile<'_>> = comments_of_project
            .iter()
            .filter_map(|cm| self.get_violation(cm))
            .collect();
        let violations_between_comments = self.get_dependecy_violations(comments_of_project);
        violations_within_comment.extend(violations_between_comments);
        violations_within_comment
    }

    fn get_dependecy_violations<'a>(
        &self,
        comments_of_project: &'a Vec<CommentData<'a>>,
    ) -> Vec<RuleViolationOnFile<'a>> {
        let comments_with_dependencies = comments_of_project
            .iter()
            .filter(|cm| !cm.dependency_list_parsed.is_empty());
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
                } else if let Some(found) = found {
                    if dep.1 != found.comment_hash_parsed {
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
        result
    }
}

fn violation_to_message(violation: &CommentIntegrityRuleViolations) -> (ViolationLevel, String) {
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
            "Code hash changed but comment was not updated".to_string(),
        ),
        CommentIntegrityRuleViolations::CommentHashNotRegenerated => (
            ViolationLevel::Warning,
            "Comment hash needs regeneration".to_string(),
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

// impl<'a> RuleViolationOnFile<'a> {
//     pub fn display_to_user(&self, output_format: &str) -> (String, String) {
//         let (level, message) = violation_to_message(&self.violation);
//         (
//             format_violation(output_format, level.as_str(), &self.comment, &message),
//             level.as_str().to_owned(),
//         )
//     }
// }
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
// pub fn display_violations_to_user(
//     violations: &[RuleViolationOnFile],
//     output_format: &str,
// ) -> String {
//     if violations.is_empty() {
//         return "No violations found! All comments are up to date.".to_string();
//     }
//
//     let mut result = format!("Found {} violation(s):\n\n", violations.len());
//
//     for (idx, violation) in violations.iter().enumerate() {
//         result.push_str(&violation.display_to_user(output_format).0);
//         if idx < violations.len() - 1 {
//             result.push_str("\n\n");
//         }
//     }
//
//     result
// }

#[derive(PartialEq, Eq, Debug, Hash, Clone, Copy)]
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

impl FromStr for CommentIntegrityRuleViolations {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use CommentIntegrityRuleViolations::*;
        match s.trim() {
            "comment-does-not-reference-specific-code" => Ok(CommentDoesNotReferenceSpecificCode),
            "parse-error-that-should-be-fixed" => Ok(ParseErrorThatShouldBeFixed),
            "code-changed-comment-not" => Ok(CodeChangedCommentNot),
            "comment-hash-not-regenerated" => Ok(CommentHashNotRegenerated),
            "comment-that-others-depend-on-changed" => Ok(CommentThatOthersDependOnChanged),
            "comment-that-others-depend-on-deleted" => Ok(CommentThatOthersDependOnDeleted),
            "depends-on-comment-that-doesnt-exist" => Ok(DependsOnCommentThatDoesntExist),
            "depends-on-comment-that-changed" => Ok(DependsOnCommentThatChanged),
            other => Err(format!("unknown violation: {other}")),
        }
    }
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
// fn get_violation<'a>(comment: &'a CommentData<'a>) -> Option<RuleViolationOnFile<'a>> {
//     if comment.should_be_ignored {
//         return None;
//     }
//
//     // Check for parse errors first
//     if let Some(ref parse_err) = comment.parse_error {
//         match parse_err {
//             StampParseError::NoStampFound => {
//                 return Some(RuleViolationOnFile {
//                     violation: CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode,
//                     comment,
//                 });
//             }
//             StampParseError::StampWithoutClosingTag
//             | StampParseError::StampWithoutLinesReferenced
//             | StampParseError::StampWithoutCodeHash => {
//                 return Some(RuleViolationOnFile {
//                     violation: CommentIntegrityRuleViolations::ParseErrorThatShouldBeFixed,
//                     comment,
//                 });
//             }
//             StampParseError::StampWithoutHashes => {
//                 // This is handled separately below in hash check
//             }
//         }
//     }
//
//     let hash_check_result = Self::check_that_stamp_is_updated(comment);
//     println!("Hash check result is: {:?}", hash_check_result);
//     match hash_check_result {
//         HashCheckResult::HashesShouldBeGenerated => Some(RuleViolationOnFile {
//             violation: CommentIntegrityRuleViolations::CommentHashNotRegenerated,
//             comment,
//         }),
//         HashCheckResult::CodeHashNotUpToDate | HashCheckResult::BothHashesInvalid => {
//             Some(RuleViolationOnFile {
//                 violation: CommentIntegrityRuleViolations::CodeChangedCommentNot,
//                 comment,
//             })
//         }
//         HashCheckResult::CommentHashNotUpToDate => Some(RuleViolationOnFile {
//             violation: CommentIntegrityRuleViolations::CommentHashNotRegenerated,
//             comment,
//         }),
//         HashCheckResult::BothHashesUpToDate => None,
//     }
// }

fn get_dependecy_violations<'a>(
    comments_of_project: &'a Vec<CommentData<'a>>,
) -> Vec<RuleViolationOnFile<'a>> {
    let comments_with_dependencies = comments_of_project
        .iter()
        .filter(|cm| !cm.dependency_list_parsed.is_empty());
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
            } else if let Some(found) = found {
                if dep.1 != found.comment_hash_parsed {
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
    result
}

// pub fn generate_violations_from_comments<'a>(
//     comments_of_project: &'a Vec<CommentData<'a>>,
// ) -> Vec<RuleViolationOnFile<'a>> {
//     let mut violations_within_comment: Vec<RuleViolationOnFile<'_>> = comments_of_project
//         .iter()
//         .filter_map(get_violation)
//         .collect();
//     let violations_between_comments = get_dependecy_violations(comments_of_project);
//     violations_within_comment.extend(violations_between_comments);
//     return violations_within_comment;
// }

#[cfg(test)]
mod tests {

    // (not yet implemented) CommentThatOthersDependOnDeleted,
    mod helpers {
        use crate::models::{self, CommentData};
        use crate::violations::ViolationChecker;

        pub fn default_checker() -> ViolationChecker {
            ViolationChecker::default()
        }

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
                CommentIntegrityRuleViolations,
                tests::helpers::{default_checker, normal_comment_with, update_hashes_like_parsed},
            },
        };
        use std::path::Path;

        #[test]
        fn if_it_does_not_reference_specific_code() {
            let mut cm = CommentData::empty();
            cm.file = Path::new("hello.js");
            let _ = cm.push_comment("//Hello World");
            let _ = cm.push_code("console.log(`Hello World`)");
            let result = default_checker()
                .get_violation(&cm)
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
            let result = default_checker().get_violation(&cm).expect(
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
            let result = default_checker().get_violation(&cm).expect(
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
            let result = default_checker().generate_violations_from_comments(&input);
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
            let result = default_checker().generate_violations_from_comments(&input);
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
            violations::tests::helpers::{
                default_checker, normal_comment_with, update_hashes_like_parsed,
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
            let result = default_checker().generate_violations_from_comments(&input);
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

            let result = default_checker().get_violation(&cm);

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

            let result = default_checker().get_violation(&cm);

            assert!(result.is_none());
        }
    }
    mod should_respect_user_config {
        use crate::{
            models::CommentData,
            violations::{
                CommentIntegrityRuleViolations::{self, *},
                ViolationChecker,
                ViolationLevel::{self, *},
                tests::helpers::update_hashes_like_parsed,
            },
        };
        use std::io::Write;

        // writes a config where everything keeps its default EXCEPT
        // code-changed-comment-not, flipped from its default Error to notice
        fn config_file_with_overridden_code_changed() -> std::path::PathBuf {
            let mut path = std::env::temp_dir();
            path.push(format!("comments_test_config_{}.txt", std::process::id()));
            let contents = "\
comment-does-not-reference-specific-code = warning
parse-error-that-should-be-fixed = error
code-changed-comment-not = notice
comment-hash-not-regenerated = warning
comment-that-others-depend-on-changed = error
comment-that-others-depend-on-deleted = error
depends-on-comment-that-doesnt-exist = error
depends-on-comment-that-changed = error
";
            let mut f = std::fs::File::create(&path).expect("create temp config");
            f.write_all(contents.as_bytes()).expect("write temp config");
            path
        }

        #[test]
        fn default_config_has_expected_levels() {
            let checker = ViolationChecker::default();
            // the ones that must stay Error (they drive exit-code FAILURE)
            assert_eq!(checker.config.level_of(CodeChangedCommentNot), Error);
            assert_eq!(checker.config.level_of(ParseErrorThatShouldBeFixed), Error);
            assert_eq!(checker.config.level_of(DependsOnCommentThatChanged), Error);
            assert_eq!(
                checker.config.level_of(CommentDoesNotReferenceSpecificCode),
                Warning
            );
        }

        #[test]
        fn changed_code_uses_overridden_level_from_file() {
            let path = config_file_with_overridden_code_changed();
            let checker = ViolationChecker::with_config_file(path.display().to_string())
                .expect("config should load");

            // same setup as the detection test: produces CodeChangedCommentNot
            let mut cm = CommentData::empty();
            let _ = cm.push_comment("//Hello World ```comments-2.0 1```");
            let _ = cm.push_code("console.log(`Hello World`)");
            let mut cm = update_hashes_like_parsed(cm);
            let _ = cm.push_code("console.log(`Hello World 2`)");
            cm.parse_error = None;

            let violation = checker
                .get_violation(&cm)
                .expect("should produce a violation");

            // sanity: it's the variant we overrode
            assert_eq!(violation.violation, CodeChangedCommentNot);

            // the point: file says notice, default would have been Error
            let level = checker.config.level_of(violation.violation);
            assert_eq!(level, Note);
            assert_ne!(level, Error); // proves it's not the default

            let _ = std::fs::remove_file(&path);
        }
    }
}
