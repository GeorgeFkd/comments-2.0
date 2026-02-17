use std::collections::HashMap;
use std::env::Args;
use std::fs::{File, read_dir};
use std::io::BufReader;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{env, fs};

use crate::models::{CommentData, StampParseError};
mod source_code_replacer;
//General Notes:
//Saving it to a db might not be ideal, just the parse the project from start everytime
//There is a file format for how github actions report errors/warnings
//I dont add warnings/errors for specific rules, leave it up to the user, will provide a sensible
//default

pub struct RuleViolationOnFile<'a> {
    violation: CommentIntegrityRuleViolations<'a>,
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
) -> (&'a CommentData<'a>, ViolationLevel, String) {
    match violation {
        CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode(comment) => (
            comment,
            ViolationLevel::Warning,
            "Comment does not reference specific code (no stamp found)".to_string(),
        ),
        CommentIntegrityRuleViolations::ParseErrorThatShouldBeFixed(comment) => (
            comment,
            ViolationLevel::Error,
            match &comment.parse_error {
                Some(StampParseError::StampWithoutClosingTag) => {
                    "stamp missing closing ```".to_string()
                }
                Some(StampParseError::StampWithoutLinesReferenced) => {
                    "stamp missing line count".to_string()
                }
                Some(StampParseError::StampWithoutCodeHash) => {
                    "stamp missing code hash".to_string()
                }
                _ => "malformed stamp".to_string(),
            },
        ),
        CommentIntegrityRuleViolations::CodeChangedCommentNot(comment) => (
            comment,
            ViolationLevel::Error,
            format!(
                "Code hash changed but comment was not updated (expected: {}, actual: {})",
                comment.code_hash_parsed,
                comment.hash_code()
            ),
        ),
        CommentIntegrityRuleViolations::CommentHashNotRegenerated(comment) => (
            comment,
            ViolationLevel::Warning,
            format!(
                "Comment hash needs regeneration (expected: {}, actual: {})",
                comment.comment_hash_parsed,
                comment.hash_comment()
            ),
        ),
        CommentIntegrityRuleViolations::CommentThatOthersDependOnChanged(comment) => (
            comment,
            ViolationLevel::Error,
            "Comment changed but other comments depend on it".to_string(),
        ),
        CommentIntegrityRuleViolations::CommentThatOthersDependOnDeleted(comment) => (
            comment,
            ViolationLevel::Error,
            "Comment deleted but other comments depend on it".to_string(),
        ),
    }
}

impl<'a> RuleViolationOnFile<'a> {
    pub fn display_to_user(&self, output_format: &str) -> (String, String) {
        let (comment, level, message) = violation_to_message(&self.violation);
        (
            format_violation(output_format, level.as_str(), comment, &message),
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
        _ => format!(
            "{}:{}:{}: {}: {}",
            comment.file.display(),
            comment.comment_location.start.row,
            comment.comment_location.start.column,
            level,
            msg
        ),
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

pub enum CommentIntegrityRuleViolations<'a> {
    CommentDoesNotReferenceSpecificCode(&'a CommentData<'a>),
    ParseErrorThatShouldBeFixed(&'a CommentData<'a>),
    CodeChangedCommentNot(&'a CommentData<'a>),
    CommentHashNotRegenerated(&'a CommentData<'a>),
    CommentThatOthersDependOnChanged(&'a CommentData<'a>),
    CommentThatOthersDependOnDeleted(&'a CommentData<'a>),
}

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

    let violations = generate_violations_from_comments(&comment_data_of_files);

    let output_format = options
        .get("output-format")
        .map(String::as_str)
        .unwrap_or("github");
    println!("Output format selected is: {:?}", output_format);

    println!(
        "=====Violations =======\n {} ",
        display_violations_to_user(violations.as_slice(), output_format)
    );
    if !violations.is_empty() {
        return std::process::ExitCode::FAILURE;
    }
    if violations
        .iter()
        .map(|rv| violation_to_message(&rv.violation))
        .any(|v| v.1 == ViolationLevel::Error)
    {
        return std::process::ExitCode::FAILURE;
    }

    let should_regenerate_non_hashed_comments = options.get("regenerate");
    if should_regenerate_non_hashed_comments.is_some() {
        println!("Generating hashes for comments that dont already have them.");

        let mut comments_by_file: HashMap<&Path, Vec<&CommentData>> = HashMap::new();
        comment_data_of_files.iter().for_each(|comment| {
            comments_by_file
                .entry(comment.file)
                .or_insert_with(Vec::new)
                .push(comment);
        });

        for (file_path, file_comments) in comments_by_file {
            let mut content_changes = Vec::new();
            for comment in file_comments {
                // Skip comments that should be ignored
                if comment.should_be_ignored {
                    continue;
                }
                if let Some(ref parse_err) = comment.parse_error {
                    if *parse_err == StampParseError::StampWithoutHashes {
                        if let Some(ref stamp_end) = comment.stamp_end {
                            content_changes.push((
                                stamp_end.row - 1, // Convert to 0-indexed
                                stamp_end.column,
                                format!(" {} {}", comment.hash_comment(), comment.hash_code()),
                            ));
                        }
                    }
                }
            }
            if content_changes.is_empty() {
                println!("No changes to be made in file {:?}\n", file_path);
                continue;
            }

            println!(
                "Adding {} hash(es) to {}",
                content_changes.len(),
                file_path.display()
            );

            let reader = BufReader::new(
                File::open(file_path).expect(&format!("File could not be opened {:?}", file_path)),
            );

            //This is a bit awkward should sometime be fixed ```comments-2.0 4 132957344 13295432557734468097```
            let content_changes_refs: Vec<(usize, usize, &str)> = content_changes
                .iter()
                .map(|(row, col, s)| (*row, *col, s.as_str()))
                .collect();

            let modified_content =
                source_code_replacer::with_multiple_added_content_at(reader, content_changes_refs)
                    .map_err(|e| format!("Failed to modify {}: {}", file_path.display(), e))
                    .unwrap();
            fs::write(file_path, modified_content)
                .map_err(|e| format!("Failed to write file {}: {}", file_path.display(), e))
                .unwrap();
        }

        println!("Hash generation complete.");
    } else {
        println!("Not generating hashes for comments that dont already have them.");
    }

    return std::process::ExitCode::SUCCESS;
    // let result = comment_data_of_files.len();
    // println!("The project comments are: {}\n", result);
    // let db_option = options.get("db");
    // let db_file = match db_option {
    //     None => "comments.sqlite".to_owned(),
    //     Some(db) => db.to_owned(),
    // };

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

fn generate_violations_from_comments<'a>(
    comments_of_project: &'a Vec<CommentData<'a>>,
) -> Vec<RuleViolationOnFile<'a>> {
    comments_of_project
        .iter()
        .filter_map(CommentData::get_violation)
        .collect()
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

mod storage {
    use super::models;
    use rusqlite::{Connection, OpenFlags, params};
    use std::path::PathBuf;

    //might add a connection with a mutex here
    pub struct SqliteDB {
        file: PathBuf,
    }

    impl SqliteDB {
        pub fn new(file: PathBuf) -> Self {
            return Self { file };
        }
    }

    pub fn store_in_sqlite(
        file: &String,
        data: &Vec<models::CommentData>,
        records_per_db_transaction: usize,
    ) -> Result<(), String> {
        let db = SqliteDB::new(file.into());
        db.store_batch(data, records_per_db_transaction)
    }

    impl Storage for SqliteDB {
        fn store_batch(
            &self,
            data: &Vec<models::CommentData<'_>>,
            records_per_transaction: usize,
        ) -> Result<(), String> {
            let conn = Connection::open_with_flags(
                &self.file,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            );
            match conn {
                Err(e) => return Err(e.to_string()),
                Ok(mut conn) => {
                    println!(
                        "A connection was successfully made in {}\n ",
                        conn.path().unwrap()
                    );

                    //i dont like the created_at timestamp it is useless, it should have an author
                    //instead and a time of change ```comments-2.0 1 12772118682245448942 12772118682245448942```
                    let initialize_db_command = conn.prepare(
                        "
CREATE TABLE IF NOT EXISTS Comments(
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    contents TEXT NOT NULL,
    code TEXT,
    contents_hash TEXT,
    code_hash TEXT,
    file_path TEXT NOT NULL,
    row INTEGER,              -- starting line or row number in file
    column INTEGER,           -- column position in file
    lines_of_code INTEGER,    -- how many lines of code the comment refers to
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);",
                    );
                    let res = initialize_db_command
                        .expect("Something went wrong when trying to create tables")
                        .execute([]);

                    let mut index_initialisation = conn.prepare(
                        "CREATE INDEX IF NOT EXISTS idx_comments_file_path ON Comments(file_path);",
                    );

                    index_initialisation
                        .expect("Something went wrong when trying to create index statement for db")
                        .execute([])
                        .expect("Could not execute index creation statement");
                    if res.is_err() {
                        return Err(res.err().unwrap().to_string());
                    }
                    //the chunk size is 100 arbitrarily, to avoid long uncommitted transactions
                    //```comments-2.0 1 16704368689317473269 16704368689317473269```
                    for chunk in data.chunks(records_per_transaction) {
                        //i manually start and stop the transaction in order to
                        //make it faster by avoiding too many transactions ```comments-2.0 1 6643353057262408975 6643353057262408975```
                        let tx = conn.transaction().unwrap();
                        {
                            let mut stmt = tx
                                .prepare(
                                    "INSERT INTO Comments
            (contents, code, contents_hash, code_hash, file_path, row, column, lines_of_code)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                                )
                                .expect("something went wrong when preparing hash");

                            for comment in chunk {
                                let file_path_str = comment.file.to_str().unwrap();

                                stmt.execute(params![
                                &comment.raw_contents,
                                &comment.code_it_refers_to,
                                &comment.hash_comment(),
                                &comment.hash_code(),
                                file_path_str,
                                comment.comment_location.start.row,
                                comment.comment_location.start.column,
                                comment.lines_of_code_referenced,
                            ])
                            .expect(
                                "Something went wrong when trying to execute an INSERT statement",
                            );
                            }
                        }
                        tx.commit().unwrap();
                    }
                    return Ok(());
                }
            }
        }

        fn store(&self, comment: &models::CommentData) -> Result<(), String> {
            let conn = Connection::open_with_flags(
                &self.file,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .expect("Something went wrong when opening the db to write");
            let mut stmt = conn
                .prepare(
                    "INSERT INTO Comments
            (contents, code, contents_hash, code_hash, file_path, row, column, lines_of_code)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )
                .expect("something went wrong when preparing hash");

            stmt.execute(params![
                &comment.raw_contents,
                &comment.code_it_refers_to,
                &comment.hash_comment(),
                &comment.hash_code(),
                comment.file.to_str().unwrap(),
                comment.comment_location.start.row,
                comment.comment_location.start.column,
                comment.lines_of_code_referenced,
            ])
            .expect("Something went wrong when trying to execute an INSERT statement");
            println!("inserted one record into db");
            return Ok(());
        }

        fn read_all(&self) -> Vec<models::CommentData> {
            todo!();
        }

        fn get_total_comments_count(&self) -> u64 {
            std::todo!();
        }

        fn get_comments_count_per_file(&self) -> (String, u64) {
            std::todo!();
        }

        fn dump_contents_human_readable(&self) -> String {
            std::todo!();
        }

        fn raw_contents(&self) -> String {
            std::todo!();
        }
    }

    pub trait Storage {
        //i might need to make this async in the future or just throw it in a different thread
        fn store(&self, data: &models::CommentData) -> Result<(), String> {
            return Ok(());
        }

        fn store_batch(
            &self,
            data: &Vec<models::CommentData>,
            records_per_transaction: usize,
        ) -> Result<(), String> {
            Ok(())
        }

        fn read_all(&self) -> Vec<models::CommentData> {
            todo!();
        }

        fn get_total_comments_count(&self) -> u64 {
            todo!();
        }

        fn get_comments_count_per_file(&self) -> (String, u64) {
            todo!();
        }

        fn dump_contents_human_readable(&self) -> String {
            todo!();
        }

        fn raw_contents(&self) -> String {
            todo!();
        }
    }
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
pub mod models {
    use std::{
        hash::{DefaultHasher, Hash, Hasher},
        path::Path,
    };

    use crate::{CommentIntegrityRuleViolations, RuleViolationOnFile};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum StampParseError {
        NoStampFound,
        StampWithoutClosingTag,
        StampWithoutLinesReferenced,
        StampWithoutHashes,
        StampWithoutCodeHash,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum HashCheckResult {
        BothHashesInvalid,
        CodeHashNotUpToDate,
        CommentHashNotUpToDate,
        BothHashesUpToDate,
        HashesShouldBeGenerated,
    }

    // i need a different struct for the parser and the db
    #[derive(Debug)]
    pub struct CommentData<'a> {
        pub comment_location: SourceRange,
        pub stamp_end: Option<SourceLocation>,
        //used in the generation of the hash
        pub raw_contents: String,
        pub code_it_refers_to: String,

        //most of the fields should be optional, to signal when they are unstamped
        pub lines_of_code_referenced: u16,
        pub should_be_ignored: bool, //when the user inputs 0 as the lines referenced i should not
        //bother with it
        pub lines_of_code_read: u16,
        pub file: &'a Path,

        pub code_hash_parsed: String,
        pub comment_hash_parsed: String,
        pub parse_error: Option<StampParseError>,
    }

    impl<'a> CommentData<'a> {
        pub fn get_violation(&self) -> Option<RuleViolationOnFile<'_>> {
            if self.should_be_ignored {
                return None;
            }

            // Check for parse errors first
            if let Some(ref parse_err) = self.parse_error {
                match parse_err {
                    StampParseError::NoStampFound => {
                        return Some(RuleViolationOnFile {
                            violation:
                                CommentIntegrityRuleViolations::CommentDoesNotReferenceSpecificCode(
                                    &self,
                                ),
                        });
                    }
                    StampParseError::StampWithoutClosingTag
                    | StampParseError::StampWithoutLinesReferenced
                    | StampParseError::StampWithoutCodeHash => {
                        return Some(RuleViolationOnFile {
                            violation: CommentIntegrityRuleViolations::ParseErrorThatShouldBeFixed(
                                &self,
                            ),
                        });
                    }
                    StampParseError::StampWithoutHashes => {
                        // This is handled separately below in hash check
                    }
                }
            }

            match self.check_that_stamp_is_updated() {
                HashCheckResult::HashesShouldBeGenerated => Some(RuleViolationOnFile {
                    violation: CommentIntegrityRuleViolations::CommentHashNotRegenerated(&self),
                }),
                HashCheckResult::CodeHashNotUpToDate | HashCheckResult::BothHashesInvalid => {
                    Some(RuleViolationOnFile {
                        violation: CommentIntegrityRuleViolations::CodeChangedCommentNot(&self),
                    })
                }
                HashCheckResult::CommentHashNotUpToDate => Some(RuleViolationOnFile {
                    violation: CommentIntegrityRuleViolations::CommentHashNotRegenerated(&self),
                }),
                HashCheckResult::BothHashesUpToDate => None,
            }
        }

        pub fn check_that_stamp_is_updated(&self) -> HashCheckResult {
            if self.parse_error.is_some()
                && self.parse_error.clone().unwrap() == StampParseError::StampWithoutHashes
            {
                return HashCheckResult::HashesShouldBeGenerated;
            }
            let code_hash_is_updated = self.hash_code() == self.code_hash_parsed;
            let comment_hash_is_updated = self.hash_comment() == self.comment_hash_parsed;
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

        pub fn empty() -> Self {
            Self {
                should_be_ignored: false,
                comment_location: SourceRange {
                    start: SourceLocation::empty(),
                    end: SourceLocation::empty(),
                },
                raw_contents: "".into(),
                file: &Path::new(""),
                code_it_refers_to: "".into(),
                lines_of_code_referenced: 0,
                lines_of_code_read: 0,
                code_hash_parsed: "".into(),
                comment_hash_parsed: "".into(),
                parse_error: Some(StampParseError::NoStampFound),
                stamp_end: None,
            }
        }

        pub fn push_comment(&mut self, string: &str) -> Option<usize> {
            //should ignore the part of the string that
            //has the comments-2.0 stamp
            //and also parse that part to see how many lines of code should be parsed next
            let open_pattern = "```comments-2.0";
            let close_pattern = "```";
            let stamp_start = string.find(open_pattern);
            // println!("Stamp start: {:?}", stamp_start);

            match stamp_start {
                None => {
                    self.raw_contents.push('\n');
                    self.raw_contents.push_str(string);
                    return None;
                }

                Some(start) => {
                    self.raw_contents.push('\n');
                    self.raw_contents.push_str(&string[0..start]);

                    let remaining = &string[start + open_pattern.len()..];
                    // println!("Remaining is: {}", remaining); ```comments-2.0 0```
                    let stamp_end = remaining.find(close_pattern);
                    if stamp_end.is_none() {
                        self.parse_error = Some(StampParseError::StampWithoutClosingTag);
                        return None;
                    }
                    //parse the slice in between
                    //in the future it is possible to just use json here and parse it with
                    //serde, not required rn
                    let stamp_slice = remaining[0..stamp_end.unwrap()].trim();
                    // println!("Stamp slice is: {:?}", stamp_slice);
                    let data: Vec<&str> = stamp_slice.split(" ").collect();
                    let parse_error = match data.len() {
                        0 => Some(StampParseError::StampWithoutLinesReferenced),
                        1 => Some(StampParseError::StampWithoutHashes),
                        2 => Some(StampParseError::StampWithoutCodeHash),
                        _ => None,
                    };
                    self.parse_error = parse_error.clone();
                    // println!("Extra data is: {:?}", data);
                    let lines_referenced = data.get(0).unwrap();
                    // println!(
                    //     "This comment references the next {} lines of code",
                    //     lines_referenced
                    // );
                    let parsed_num = lines_referenced.parse().expect(
                        &format!("Could not parse the lines of code number of the ```comments-2.0 ``` stamp at {}:{}:{}",self.file.display(),self.comment_location.start.row,self.comment_location.start.column
                    ));
                    self.lines_of_code_referenced = parsed_num;
                    //user wants us to ignore this comment ```comments-2.0 3 427109853614679882 427109853614679882```
                    if parsed_num == 0 {
                        self.should_be_ignored = true;
                    }

                    if parse_error.is_some() {
                        return Some(start + open_pattern.len() + stamp_end.unwrap());
                    }

                    let hash_of_comment = data.get(1).unwrap();
                    self.comment_hash_parsed = hash_of_comment.trim().to_string();
                    // println!(
                    //     "This comment is already stamped with comment hash, the hash is: {}",
                    //     hash_of_comment
                    // );

                    let hash_of_code = data.get(2).unwrap();
                    self.code_hash_parsed = hash_of_code.trim().to_string();
                    // println!(
                    //     "This comment is already stamped with code hash, the hash is: {}",
                    //     hash_of_code
                    // );
                    return Some(start + open_pattern.len() + stamp_end.unwrap());
                }
            }
        }

        pub fn push_code(&mut self, string: &str) {
            self.code_it_refers_to.push_str(string);
            self.code_it_refers_to.push('\n');
        }
    }

    impl<'a> CommentData<'a> {
        pub fn hash_comment(&self) -> String {
            //using only the code contents and the filename, also the content should first be split into
            //words, also certain characters should be ignored. ```comments-2.0 1 15156721570910937981 15156721570910937981```
            let mut state = DefaultHasher::new();
            let normalized: Vec<String> = self
                .raw_contents
                .split_whitespace() // split into words
                .map(|word| {
                    word.chars()
                        .filter(|c| c.is_alphanumeric()) // keep only alphanumeric
                        .collect::<String>()
                })
                .filter(|s| !s.is_empty()) // skip empty words
                .collect();

            for word in normalized {
                word.hash(&mut state);
            }

            self.file.hash(&mut state);
            return state.finish().to_string();
        }

        //using only the code contents and the filename, also the content should first be split into
        //words, also certain characters should be ignored. ```comments-2.0 1 15156721570910937981 15156721570910937981```
        pub fn hash_code(&self) -> String {
            //might need to implement a custom one at some point ```comments-2.0 1 5703807205826246641 5703807205826246641```
            let mut state = DefaultHasher::new();
            let normalized: Vec<String> = self
                .raw_contents
                .split_whitespace() // split into words
                .map(|word| {
                    word.chars()
                        .filter(|c| c.is_alphanumeric()) // keep only alphanumeric
                        .collect::<String>()
                })
                .filter(|s| !s.is_empty()) // skip empty words
                .collect();

            for word in normalized {
                word.hash(&mut state);
            }

            self.file.hash(&mut state);
            return state.finish().to_string();
        }
    }

    #[derive(Debug)]
    pub struct SourceLocation {
        pub row: usize,
        //column will now not be used yet
        pub column: usize,
    }

    impl SourceLocation {
        pub fn empty() -> Self {
            Self { row: 0, column: 0 }
        }
    }

    #[derive(Debug)]
    pub struct SourceRange {
        pub start: SourceLocation,
        pub end: SourceLocation,
    }
}

#[cfg(test)]
mod tests {

    mod comments {
        use crate::{CommentData, models::StampParseError};

        fn empty_comment<'a>() -> CommentData<'a> {
            CommentData::empty()
        }

        #[test]
        fn comment_with_valid_hashes_is_ok() {
            let mut cm = empty_comment();
            let _ = cm.push_comment(
                "This is a comment ```comments-2.0 1 11816667893181836463 11816667893181836463```",
            );
            cm.push_code("console.log(`hello world`);");

            assert!(cm.parse_error.is_none());
            assert_eq!(cm.lines_of_code_referenced, 1);
            assert_eq!(cm.comment_hash_parsed, "11816667893181836463");
            assert_eq!(cm.code_hash_parsed, "11816667893181836463");
            assert!(cm.code_it_refers_to.contains("console.log"));
        }

        #[test]
        fn comment_without_stamp_is_reported() {
            let mut cm = empty_comment();
            let _ = cm.push_comment("a comment without a stamp at all");

            assert!(cm.parse_error.is_some());
            assert_eq!(cm.parse_error.unwrap(), StampParseError::NoStampFound);
        }

        #[test]
        fn comment_without_closing_tag_is_reported() {
            let mut cm = empty_comment();
            let _ = cm.push_comment("a comment without a closing tag ```comments-2.0 2 abc defg");

            assert!(cm.parse_error.is_some());
            assert_eq!(
                cm.parse_error.unwrap(),
                StampParseError::StampWithoutClosingTag
            );
        }

        #[test]
        fn comment_without_any_of_the_hashes_is_reported() {
            let mut cm = empty_comment();
            let _ = cm.push_comment("hello world ```comments-2.0 1```");

            let mut cm2 = empty_comment();
            let _ = cm2.push_comment("hello world ```comments-2.0 1 abc ```");

            assert!(cm.parse_error.is_some());
            assert_eq!(cm.parse_error.unwrap(), StampParseError::StampWithoutHashes);

            assert!(cm2.parse_error.is_some());
            assert_eq!(
                cm2.parse_error.unwrap(),
                StampParseError::StampWithoutCodeHash
            );
        }

        #[test]
        fn comment_with_both_hashes_has_no_parse_error() {
            let mut cm = empty_comment();
            let _ = cm.push_comment("hello world ```comments-2.0 1 abc defg  ```");

            assert!(cm.parse_error.is_none());
        }

        #[test]
        fn multiline_comment_on_same_line_gets_stamp_correctly() {
            let mut cm = empty_comment();
            let _ = cm.push_comment("hello there ```comments-2.0 1 abc defg``` */");

            assert_eq!(cm.comment_hash_parsed, "abc");
            assert_eq!(cm.code_hash_parsed, "defg");
        }

        #[test]
        fn multiline_comment_gets_stamp_correctly() {
            let mut cm = empty_comment();
            let _ = cm.push_comment(
                "
hello world ```comments-2.0 2 abc defg``` 
hello there */
",
            );
            assert_eq!(cm.comment_hash_parsed, "abc");
            assert_eq!(cm.code_hash_parsed, "defg");
        }

        #[test]
        fn single_line_comment_gets_stamp_correctly() {
            let mut cm = empty_comment();
            let _ = cm.push_comment("hello world ```comments-2.0 1 abc defg ```");

            assert_eq!(cm.comment_hash_parsed, "abc");
            assert_eq!(cm.code_hash_parsed, "defg");
        }
    }

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

