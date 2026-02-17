use std::collections::HashMap;
use std::env::Args;
use std::fs::{File, read_dir};
use std::io::BufReader;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{env, fs};
mod models;

use crate::models::{CommentData, StampParseError};
use crate::violations::RuleViolationOnFile;
mod source_code_replacer;
//General Notes:
//Saving it to a db might not be ideal, just the parse the project from start everytime
//There is a file format for how github actions report errors/warnings
//I dont add warnings/errors for specific rules, leave it up to the user, will provide a sensible
//default
mod storage;
mod violations;

fn position_from_row_col(content: &str, row: u64, col: u64) -> Option<usize> {
    let mut current_row = 0u64;
    let mut position = 0usize;

    for line in content.lines() {
        current_row += 1;

        if current_row == row {
            // Found the target row
            if col as usize > line.len() {
                return None; // Column out of bounds
            }
            return Some(position + col as usize);
        }

        // Add line length + newline character
        position += line.len() + 1;
    }

    None // Row not found
}

//this might become an enum ```comments-2.0 1 1489893222977162209 148989362209```
type AppError = String;

type AppResult<'a> = Result<Vec<RuleViolationOnFile<'a>>, AppError>;

fn group_comments_by_file<'a>(
    comments: impl Iterator<Item = &'a CommentData<'a>>,
) -> HashMap<&'a Path, Vec<&'a CommentData<'a>>> {
    let mut result: HashMap<&Path, Vec<&CommentData>> = HashMap::new();
    comments.for_each(|comment| {
        result
            .entry(comment.file)
            .or_insert_with(Vec::new)
            .push(comment);
    });
    result
}

fn main() -> std::process::ExitCode {
    let program_args = env::args();

    let options = parse_program_args(program_args)
        .inspect_err(|e| eprintln!("Error while parsing args: {e}"))
        .unwrap();

    assert!(options.len() > 0);

    match are_args_valid(&options) {
        Ok(()) => println!("Check: Arguments are valid"),
        Err(e) => {
            eprintln!("Error while checking option combinations validity {e}");
            eprintln!("{}", format!("{}", help_page().as_str()));
        }
    }

    let file_extensions = options
        .get("file-extensions")
        .expect("should provide the --file-extensions flag");
    let file_extensions: Vec<String> = file_extensions
        .trim()
        .split(",")
        .map(String::from)
        .collect();

    //for each language i could write the ignored-dirs myself ```comments-2.0 3 17485437293245936657 17485437293245936657```
    let ignored_dirs = options
        .get("ignored-dirs")
        .expect("should provide the --ignored-dirs flag");
    let ignored_dirs: Vec<String> = ignored_dirs.trim().split(",").map(String::from).collect();
    let project_files = get_files_from_directory_recursively(
        options
            .get("source")
            .expect("should provide --source flag")
            .into(),
        &ignored_dirs,
        &file_extensions,
    );

    println!("Will process {} project files", project_files.len());
    let threads = get_threads_to_use(project_files.len() as u64);
    if threads.is_some() {
        println!("Will use {} threads", threads.unwrap());
    } else {
        println!("Will be single threaded")
    }

    let start = Instant::now();

    let comment_data_of_files: Vec<models::CommentData> = project_files
        .iter()
        .flat_map(|p| parser::parse_file(p, BufReader::new(File::open(p).unwrap())))
        .collect();
    let end = Instant::now();
    //50k files in 13 seconds for the llvm project with some dirs excluded
    println!(
        "Completed comments parsing {} files in {:?}",
        project_files.len(),
        end.duration_since(start)
    );

    let violations = violations::generate_violations_from_comments(&comment_data_of_files);

    let output_format = options
        .get("output-format")
        .map(String::as_str)
        .unwrap_or("github");
    println!("Output format selected is: {:?}", output_format);

    println!(
        "=====Violations =======\n {} ",
        violations::display_violations_to_user(violations.as_slice(), output_format)
    );

    let should_regenerate_non_hashed_comments = options.get("regenerate");
    if should_regenerate_non_hashed_comments.is_some() {
        println!("Generating hashes for comments that dont already have them.");
        let comments_per_file = group_comments_by_file(comment_data_of_files.iter());
        if let Err(e) =
            source_code_replacer::regenerate_hashes_in_files(comments_per_file.into_iter())
        {
            eprintln!("Something went wrong when generating the hashes for files: {e}");
        }

        println!("Hash generation complete.");
    } else {
        println!("Not generating hashes for comments that dont already have them.");
    }

    return violations::determine_exit_code(violations.as_slice());
    // let result = comment_data_of_files.len();
    // println!("The project comments are: {}\n", result);
    // let db_option = options.get("db");
    // let db_file = match db_option {
    //     None => "comments.sqlite".to_owned(),
    //     Some(db) => db.to_owned(),
    // };
    //
    // println!("Storing them in db: {db_file}\n");
    // let start = Instant::now();
    // let result = storage::store_in_sqlite(&db_file, &comment_data_of_files, 500);
    // if result.is_err() {
    //     println!("Something went wrong when trying to store data in the database");
    //     return std::process::ExitCode::FAILURE;
    // } else {
    //     let end = Instant::now();
    //     println!(
    //         "Storing them to sqlite needed {:?}",
    //         end.duration_since(start)
    //     );
    //     return std::process::ExitCode::SUCCESS;
    // }
}

fn help_page() -> String {
    "
USAGE:
    $EXEC --source <PATH> --file-extensions <EXTS> --ignored-dirs <DIRS> [OPTIONS]

DESCRIPTION:
    A tool to parse and track code comments that reference specific lines of code.
    Comments can be 'stamped' with ```comments-2.0 N COMMENT_HASH CODE_HASH``` to 
    indicate they reference the next N lines of code and include integrity hashes.
    
    The tool stores can detect:
    - Comments without stamps (unstamped comments)
    - Code that changed but comment didn't (stale comments)
    - Comments that changed but code didn't 
    - Dependencies between comments, and generate warnings when a dependency is deleted or changed

REQUIRED FLAGS:
    --source <PATH>
        Path to the source code directory to analyze

    --file-extensions <EXTS>
        Comma-separated list of file extensions to process
        Example: --file-extensions rs,js,cpp

    --ignored-dirs <DIRS>
        Comma-separated list of directory names to skip during traversal
        Example: --ignored-dirs node_modules,target,build

OPTIONAL FLAGS:
    --db <PATH>
        Path to SQLite database file (default: comments.sqlite)
    --regenerate
        Flag to generate hashes for comments that have not be hashed yet
    --output-format <github>
        How to print out the violations
COMMENT STAMP FORMAT:
    Single-line comments:
        // Your comment text ```comments-2.0 0 comment_hash code_hash```
        
    Multi-line comments:
        /* Your comment text
           across multiple lines
```comments-2.0 0 comment_hash code_hash```
        */

    Where:
        N(=0)            = Number of lines of code this comment references
        COMMENT_HASH = Hash of the comment content (auto-generated)
        CODE_HASH    = Hash of the referenced code (auto-generated)

    Special cases:
        - N=0: Comment will be ignored in integrity checks
        - Missing hashes: Tool will flag for hash generation
        - Mismatched hashes: Tool will flag as code or comment changed
        - Missing stamp: The violation will be reported on tool run
INTEGRITY CHECKS:
    The tool performs the following checks:
    ✓ Detects comments without stamps
    ✓ Detects stamps without hashes (needs generation)
    ✓ Detects code changes (code hash mismatch)
    ✓ Detects comment changes (comment hash mismatch)
    ✓ Tracks location information (file, row, column)

EXAMPLES:
    # Analyze a Rust project
    $EXEC --source ./my-project --file-extensions rs --ignored-dirs target

    # Analyze multiple file types with custom database
    $EXEC --source ./codebase --file-extensions js,ts,jsx --ignored-dirs node_modules,dist --db ./my-comments.db

    # Example properly stamped comment in source code:
    // This function validates user input by trimming whitespace
    // and checking minimum length requirements
    //     ```comments-2.0 3 1234567890 0987654321```
    function validateInput(data) {
        const trimmed = data.trim();
        return trimmed.length > 0;
    }

    # Example comment to be ignored (N=0):
    // General note about the file architecture
    // ```comments-2.0 0```

".to_string()
}

fn get_threads_to_use(files_to_process: u64) -> Option<usize> {
    if files_to_process < 1000 {
        return None;
    };
    let threads: usize = std::thread::available_parallelism().unwrap().into();
    println!("Logical cpus are: {threads}");

    let os = env::consts::OS;
    assert!(os == "linux");
    let meminfo_contents = fs::read_to_string(Path::new("/proc/meminfo")).unwrap();
    //the records are in the form <label>:\t number kB
    let available_memory = meminfo_contents
        .lines()
        .into_iter()
        .find(|l| l.starts_with("MemAvailable"))
        .unwrap()
        .split(":")
        .skip(1)
        .take(1)
        .last()
        .unwrap();
    let available_memory: Vec<&str> = available_memory.trim_start().split(" ").collect();
    let available_memory: usize = available_memory
        .get(0)
        .unwrap()
        .parse()
        .expect("Could not parse /proc/meminfo file");
    println!("System has {available_memory} kB memory");

    return Some(threads);
}

fn get_files_from_directory_recursively(
    dir: PathBuf,
    ignored_dirs: &Vec<String>,
    file_extensions_allowed: &Vec<String>,
) -> Vec<PathBuf> {
    //the performance might be bad
    assert!(dir.is_dir());
    match read_dir(dir) {
        Err(e) => vec![],
        Ok(entries) => entries
            .filter(|p| p.is_ok())
            .flat_map(|p| {
                let p = p.unwrap().path();
                let last_path = p.as_path().iter().last().unwrap().to_str().unwrap();
                match p.is_dir()
                    && !ignored_dirs.contains(&last_path.to_owned())
                    && !last_path.starts_with(".")
                {
                    true => get_files_from_directory_recursively(
                        p,
                        ignored_dirs,
                        file_extensions_allowed,
                    ),
                    false => {
                        let mut result = vec![];
                        if let Some(v) = p.extension() {
                            if file_extensions_allowed.contains(&v.to_str().unwrap().to_owned()) {
                                result.push(p);
                            }
                        }
                        return result;
                    }
                }
            })
            .collect(),
    }
}

fn parse_program_args(args: Args) -> Result<HashMap<String, String>, String> {
    //the format is: --<argname1><space><value><space>--<argname2>
    //no need for a library

    let mut args: Vec<String> = args.collect();
    if args.len() == 1 {
        let help_msg = format!(
            "No arguments passed to executable, the help can be seen here: \n {}",
            help_page()
        );
        return Err(help_msg);
    }

    let mut args = args.into_iter();

    let mut result = HashMap::new();
    args.skip(1)
        .reduce(|acc, s| return String::from(acc) + " " + &s)
        .expect("It was previously checked that we have enough arguments")
        .split("--")
        .skip(1)
        .for_each(|arg| {
            let mut key_val_pair = arg.split(" ");
            let key = key_val_pair.next().unwrap();
            let val = key_val_pair.next();
            match val {
                None => result.insert(key.to_owned(), "".to_owned()),
                Some(v) => result.insert(key.to_owned(), v.to_owned()),
            };
        });

    return Ok(result);
}

fn are_args_valid(args: &HashMap<String, String>) -> Result<(), &'static str> {
    //this is just some business logic for validating mutually exclusive params etc. etc.
    return Ok(());
}

mod parser {
    use crate::models::{CommentData, SourceLocation};

    use std::io::prelude::*;
    use std::path::Path;

    fn project_folder() -> String {
        return "".into();
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

        for line in reader.lines().flatten() {
            current_row += 1;

            // Calculate column (where non-whitespace starts)
            let leading_whitespace = line.len() - line.trim_start().len();
            let current_column = leading_whitespace;
            let trimmed = line.trim_start();

            match state {
                State::Code => {
                    if trimmed.starts_with("/*") {
                        state = State::MultiLineComment;
                        current_comment.comment_location.start.row = current_row;
                        current_comment.comment_location.start.column = current_column;
                        let ind = current_comment.push_comment(&trimmed["/*".len()..]);
                        if ind.is_some() {
                            let pos_in_comment = ind.unwrap();
                            let pos_in_trimmed = "/*".len() + pos_in_comment;
                            let pos_in_original_line = current_column as usize + pos_in_trimmed;
                            current_comment.stamp_end = Some(SourceLocation {
                                row: current_row,
                                column: pos_in_original_line,
                            });
                        }
                    } else if trimmed.starts_with("//") {
                        state = State::SingleLineComment;
                        current_comment.comment_location.start.row = current_row;
                        current_comment.comment_location.start.column = current_column;
                        let ind = current_comment.push_comment(&trimmed["//".len()..]);
                        if ind.is_some() {
                            let pos_in_comment = ind.unwrap();
                            let pos_in_trimmed = "//".len() + pos_in_comment;
                            let pos_in_original_line = current_column as usize + pos_in_trimmed;
                            current_comment.stamp_end = Some(SourceLocation {
                                row: current_row,
                                column: pos_in_original_line,
                            });
                        }
                    }
                }

                State::SingleLineComment => {
                    if trimmed.starts_with("//") {
                        let ind = current_comment.push_comment(&trimmed["//".len()..]);
                        if ind.is_some() {
                            let pos_in_comment = ind.unwrap();
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
                                    remaining: current_comment.lines_of_code_referenced as usize
                                        - 1,
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
                        if ind.is_some() {
                            let pos_in_comment = ind.unwrap();
                            let pos_in_original_line = current_column as usize + pos_in_comment;
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
}

#[cfg(test)]
mod tests {
    mod parser {

        use crate::{
            models::{self, StampParseError},
            parser::parse_file,
            source_code_replacer::with_multiple_added_content_at,
        };

        use std::{io::BufReader, path::Path};

        fn parse_file_helper(file_contents: &str) -> Vec<models::CommentData<'_>> {
            parse_file(
                Path::new("a_random_file.js"),
                BufReader::new(file_contents.as_bytes()),
            )
        }
        #[test]
        fn insert_hashes_at_stamp_end_position() {
            let file_contents = "//This is a comment ```comments-2.0 1```
console.log(`hello world`);
";
            let result = parse_file_helper(file_contents);

            assert_eq!(result.len(), 1);
            let comment = &result[0];

            // Verify we have stamp_end set
            assert!(comment.stamp_end.is_some());
            let stamp_end = comment.stamp_end.as_ref().unwrap();

            // Generate the hashes to insert
            let comment_hash = comment.hash_comment();
            let code_hash = comment.hash_code();
            let hashes_to_insert = format!(" {} {}", comment_hash, code_hash);

            // Use with_multiple_added_content_at to insert the hashes
            let reader = BufReader::new(file_contents.as_bytes());
            let modified_content = with_multiple_added_content_at(
                reader,
                vec![(
                    stamp_end.row - 1,
                    stamp_end.column,
                    hashes_to_insert.as_str(),
                )],
            )
            .expect("Should successfully insert hashes");

            println!("Original:\n{}", file_contents);
            println!("Modified:\n{}", modified_content);

            // Verify the hashes were inserted correctly
            let modified_line = modified_content.lines().next().unwrap();
            assert!(modified_line.contains(&comment_hash));
            assert!(modified_line.contains(&code_hash));

            // Verify the format is correct: ```comments-2.0 1 COMMENT_HASH CODE_HASH```
            let expected_stamp = format!("```comments-2.0 1 {} {}```", comment_hash, code_hash);
            assert!(modified_line.contains(&expected_stamp));

            // Verify the rest of the line is unchanged
            assert!(modified_line.starts_with("//This is a comment"));
            assert!(modified_line.ends_with("```"));
        }
        #[test]
        fn find_correct_insert_position_for_stamp_hashes() {
            let file_contents = "//This is a comment ```comments-2.0 1```
console.log(`hello world`);
";
            let result = parse_file_helper(file_contents);

            assert_eq!(result.len(), 1);
            let comment = &result[0];

            // This comment should have parse error for missing hashes
            assert!(comment.parse_error.is_some());
            assert_eq!(
                comment.parse_error,
                Some(StampParseError::StampWithoutHashes)
            );

            assert!(
                comment.stamp_end.is_some(),
                "The stamp_end should be set to where we need to insert hashes."
            );
            let stamp_end = comment.stamp_end.as_ref().unwrap();
            let expected_insert_position = file_contents.find("1```").unwrap() + 1;

            // Verify the position is correct
            let line = file_contents.lines().next().unwrap();
            assert_eq!(
                &line[expected_insert_position..expected_insert_position + 3],
                "```"
            );
            assert_eq!(
                &line[expected_insert_position - 1..expected_insert_position],
                "1"
            );

            // Verify stamp_end matches our expected position
            assert_eq!(stamp_end.row, 1);
            assert_eq!(stamp_end.column as usize, expected_insert_position);

            println!(
                "Insert position from stamp_end: row {}, col {}",
                stamp_end.row, stamp_end.column
            );
            println!("Should insert hashes between '1' and '```'");
        }

        #[test]
        fn happy_path_single_line_comment() {
            let file_contents = "//this is a comment ```comments-2.0 1```
console.log(`hello world`);
";
            let result = parse_file_helper(file_contents);

            let all_have_file = result
                .iter()
                .all(|comments| !comments.file.to_str().unwrap().is_empty());
            assert!(all_have_file);
            assert_eq!(result.len(), 1);
        }

        #[test]
        fn happy_path_group_of_single_line_comments() {
            let file_contents = "//this is a group of single line comments
//that continues to the next line ```comments-2.0 1 8584938990732183766 8584938990732183766```
console.log(`hello world`);
";

            let result = parse_file_helper(file_contents);

            assert_eq!(result.len(), 1);
            let comment = result.get(0);
            assert!(comment.is_some());
            assert!(
                comment
                    .unwrap()
                    .raw_contents
                    .contains("that continues to the next line")
            );
        }

        #[test]
        fn comment_that_refs_zero_lines_is_ignored() {
            let file_contents = "//intentionally unstamped ```comments-2.0 0```
code that should not be captured";

            let result = parse_file_helper(file_contents);

            assert!(result.len() == 1);
            assert_eq!(result[0].lines_of_code_referenced, 0);
            assert!(result[0].should_be_ignored == true);
            assert!(result[0].code_it_refers_to.is_empty());
        }

        #[test]
        fn location_tracking_is_accurate() {
            let file_contents = "
//comment on line 2 ```comments-2.0 1 12406043342562534191 12406043342562534191```
    code with indent on line 3
";

            let result = parse_file_helper(file_contents);
            assert!(result.len() == 1);
            assert_eq!(result[0].comment_location.start.row, 2);
            assert_eq!(result[0].comment_location.start.column, 0);
            assert_eq!(result[0].comment_location.end.row, 2);
        }

        #[test]
        fn consecutive_ultiline_comments_are_grouped() {
            let file_contents = "/* comment 1 */
/* comment 2 ```comments-2.0 1``` */
code";

            let result = parse_file_helper(file_contents);

            assert_eq!(result.len(), 1);
        }

        #[test]
        fn can_detect_multiline_comments() {
            let file_contents = "/*
this is a multiline comment
that expands to multiple lines
```comments-2.0 1 5210024978214657710 5210024978214657710``` 
*/
console.log(`hello world`);
";

            let result = parse_file_helper(file_contents);

            assert_eq!(result.len(), 1);
            let comment = result.get(0);
            assert!(comment.is_some());
            assert!(
                comment
                    .unwrap()
                    .raw_contents
                    .contains("that expands to multiple lines")
            );
        }

        #[test]
        fn can_handle_all_types_of_comments() {
            let file_contents = "// single line comment ```comments-2.0 1```
console.log(`hello world`)

//group of single line comments
//that should be considered one ```comments-2.0 2 5797501905077812981 5797501905077812981```
console.log(`Line 1`)
console.log(`Line 2`)

/* a multiline comment
that has many more lines
and it doesnt end really

```comments-2.0 2 7754354514631241609 7754354514631241609```
*/
console.log(`Line 3`)
console.log(`Line 4`)
";

            let result = parse_file_helper(file_contents);

            assert_eq!(result.len(), 3);

            for comment in result {
                println!("Comment: {}", &comment.code_it_refers_to);
                let has_code =
                    comment.lines_of_code_referenced > 0 && comment.code_it_refers_to.len() > 0;
                let has_comment = comment.raw_contents.len() > 0;
                let has_file_path = comment.file.to_str().unwrap().len() > 0;
                assert!(has_comment && has_file_path);
            }
        }

        #[test]
        fn ignores_inline_comments() {
            let file_contents =
                "console.log(`Hello World`); //this prints hello world to the console";
            let extra_example = "console.log(/* args: */ `Hello World`);";
            let result = parse_file_helper(file_contents);

            let result_for_comment_inside_code = parse_file_helper(extra_example);

            assert!(result_for_comment_inside_code.len() == 0);
            assert!(result.len() == 0);
        }

        #[test]
        fn empty_comments_should_be_detected() {
            let file_contents = "// single line comment ```comments-2.0 1```
console.log(`hello world`)

//group of single line comments
//that should be considered one ```comments-2.0 2 5797501905077812981 5797501905077812981```
console.log(`Line 1`)
console.log(`Line 2`)

/* a multiline comment
that has many more lines
and it doesnt end really

```comments-2.0 2 7754354514631241609 7754354514631241609```
*/
console.log(`Line 3`)
console.log(`Line 4`)

//
";

            let result = parse_file_helper(file_contents);
            assert_eq!(result.len(), 4);
            assert!(result.iter().all(|comment| comment.raw_contents.len() > 0));
        }
    }
    // #[test]
    // fn rejects_invalid_arguments() {
    //     //this can also be a test-doc how's it called in rust
    //     todo!();
    // }
}
