use std::collections::HashMap;
use std::env::Args;
use std::fs::{File, read_dir};
use std::io::BufReader;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{env, fs};

fn main() -> std::process::ExitCode {
    let program_args = env::args();

    let options = parse_program_args(program_args)
        .inspect_err(|e| eprintln!("Error while parsing args: {e}"))
        .unwrap();

    assert!(options.len() > 0);

    match are_args_valid(&options) {
        Ok(()) => println!("Check: Arguments are valid"),
        Err(e) => eprintln!("Error while checking option combinations validity {e}"),
    }

    let file_extensions = options
        .get("file-extensions")
        .expect("should provide the --file-extensions flag");
    let file_extensions: Vec<String> = file_extensions
        .trim()
        .split(",")
        .map(String::from)
        .collect();

    //for each language i could write the ignored-dirs myself ```comments-2.0 3```
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
    //50k files in 13 seconds
    println!(
        "Completed comments parsing in {:?}",
        end.duration_since(start)
    );

    let result = comment_data_of_files.len();
    println!("The project comments are: {}\n", result);
    let db_option = options.get("db");
    let db_file = match db_option {
        None => "comments.sqlite".to_owned(),
        Some(db) => db.to_owned(),
    };

    println!("Storing them in db: {db_file}\n");
    let start = Instant::now();
    let result = storage::store_in_sqlite(&db_file, &comment_data_of_files, 500);
    if result.is_err() {
        println!("Something went wrong when trying to store data in the database");
        return std::process::ExitCode::FAILURE;
    } else {
        let end = Instant::now();
        println!(
            "Storing them to sqlite needed {:?}",
            end.duration_since(start)
        );
        return std::process::ExitCode::SUCCESS;
    }
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

fn parse_program_args(args: Args) -> Result<HashMap<String, String>, &'static str> {
    //the format is: --<argname1><space><value><space>--<argname2>
    //no need for a library

    let mut args: Vec<String> = args.collect();
    if args.len() == 1 {
        return Err("No arguments passed to executable, can display help page here.");
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
                    //instead and a time of change ```comments-2.0 1```
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
                    //```comments-2.0 1```
                    for chunk in data.chunks(records_per_transaction) {
                        //i manually start and stop the transaction in order to
                        //make it faster by avoiding too many transactions ```comments-2.0 1```
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
                                comment.location.start.row,
                                comment.location.start.column,
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
                comment.location.start.row,
                comment.location.start.column,
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

    use super::models;
    use std::io::prelude::*;
    use std::path::Path;

    //we will be ignoring the inline comments probably
    #[derive(PartialEq, Eq, Copy, Clone)]
    pub enum ParserPositionType {
        // has seen the character /*
        IN_MULTILINE_COMMENT,
        // has seen the character //
        IN_SINGLE_LINE_COMMENT,
        // seen the character // not at the beginning of the line
        IN_INLINE_COMMENT,
        // is just parsing normally, after single line comment and newline or
        // after the */ multiline comments
        NOT_IN_A_COMMENT,
        NONE,
    }

    fn project_folder() -> String {
        return "".into();
    }

    struct ParserState {
        pub previous: ParserPositionType,
        pub position: ParserPositionType,
        pub location: models::SourceLocation,
    }

    impl ParserState {
        pub fn empty() -> Self {
            Self {
                previous: ParserPositionType::NONE,
                position: ParserPositionType::NONE,
                location: SourceLocation::empty(),
            }
        }

        pub fn set_state(&mut self, state: ParserPositionType) {
            self.previous = self.position;
            self.position = state;
        }
    }

    pub fn parse_file<T: BufRead>(file: &Path, reader: T) -> Vec<CommentData<'_>> {
        let mut parser_state = ParserState::empty();
        let mut current_comment_data = CommentData::empty();
        let mut result = Vec::new();
        current_comment_data.file = file;
        let mut current_row = 0;
        //for this the performance can be improved by going for lower-level parser stuff
        //i also need the lower-level parser stuff in order to implement editing in file
        for l in reader.lines() {
            current_row += 1;
            if l.is_err() {
                break;
            }
            let current_line = l.unwrap();
            let current_line = current_line.trim_start();
            if current_line.starts_with("/*") {
                parser_state.set_state(ParserPositionType::IN_MULTILINE_COMMENT);
                let comment: String = current_line.to_string().chars().skip("/*".len()).collect();
                current_comment_data.push_comment(&comment);
                current_comment_data.location.start.row = current_row;
                continue;
            }
            if current_line.starts_with("//") {
                let comment: String = current_line.to_string().chars().skip("//".len()).collect();
                current_comment_data.push_comment(&comment);
                current_comment_data.location.start.row = current_row;
                parser_state.set_state(ParserPositionType::IN_SINGLE_LINE_COMMENT);
                continue;
            }
            if current_line.starts_with("*/") {
                parser_state.set_state(ParserPositionType::NONE);
                continue;
            }
            if parser_state.position == ParserPositionType::IN_MULTILINE_COMMENT {
                current_comment_data.push_comment(current_line.trim_start());
                continue;
            }

            if parser_state.position != ParserPositionType::NOT_IN_A_COMMENT {
                parser_state.set_state(ParserPositionType::NOT_IN_A_COMMENT);
            }

            if parser_state.previous == ParserPositionType::IN_SINGLE_LINE_COMMENT
                || parser_state.previous == ParserPositionType::IN_MULTILINE_COMMENT
            {
                let is_stamped = current_comment_data.lines_of_code_referenced > 0;
                if !is_stamped {
                    //an unstamped comment does not need lines of code, just add it to the db
                    //```comments-2.0 1```
                    result.push(current_comment_data);
                    current_comment_data = CommentData::empty();
                    current_comment_data.file = file;
                    //this is important as if we dont do this we just keep adding comments
                    //for every line of code ```comments-2.0 1```
                    parser_state.set_state(ParserPositionType::NOT_IN_A_COMMENT);
                    continue;
                };

                let has_lines_of_code_left_to_read = current_comment_data.lines_of_code_read
                    != current_comment_data.lines_of_code_referenced;
                if !has_lines_of_code_left_to_read {
                    result.push(current_comment_data);
                    current_comment_data = CommentData::empty();
                    current_comment_data.file = file;
                } else {
                    current_comment_data.lines_of_code_read += 1;
                    current_comment_data.push_code(current_line.trim_start());
                }
            }
        }
        //when it finishes we just pick up any relevant comment ```comments-2.0 3```
        if current_comment_data.raw_contents.len() > 0 {
            result.push(current_comment_data);
        }

        //for some reason empty comments are generated in this file and need to be filtered out
        //```comments-2.0 4```
        return result
            .into_iter()
            .filter(|comment| comment.raw_contents.len() > 0)
            .collect();
    }
}
pub mod models {
    use std::{
        hash::{DefaultHasher, Hash, Hasher},
        path::Path,
    };

    // i need a different struct for the parser and the db
    pub struct CommentData<'a> {
        pub location: SourceRange,
        //used in the generation of the hash
        pub raw_contents: String,
        pub code_it_refers_to: String,

        //most of the fields should be optional, to signal when they are unstamped
        pub lines_of_code_referenced: u16,
        pub lines_of_code_read: u16,
        pub file: &'a Path,
    }

    impl<'a> CommentData<'a> {
        pub fn empty() -> Self {
            Self {
                location: SourceRange {
                    start: SourceLocation { row: 0, column: 0 },
                    end: SourceLocation { row: 0, column: 0 },
                },
                raw_contents: "".into(),
                file: &Path::new(""),
                code_it_refers_to: "".into(),
                lines_of_code_referenced: 0,
                lines_of_code_read: 0,
            }
        }

        pub fn push_comment(&mut self, string: &str) {
            //should ignore the part of the string that
            //has the comments-2.0 stamp
            //and also parse that part to see how many lines of code should be parsed next
            let open_pattern = "```comments-2.0";
            let close_pattern = "```";
            let stamp_start = string.find(open_pattern);

            match stamp_start {
                Some(start) => {
                    let remaining = &string[start + open_pattern.len()..];
                    let stamp_end = remaining
                        .find(close_pattern)
                        .expect("```comments-2.0 ``` stamp is supposed to be all in the same line");
                    if start != stamp_end {
                        self.raw_contents.push('\n');
                        self.raw_contents.push_str(&string[0..start]);
                        //parse the slice in between
                        //in the future it is possible to just use json here and parse it with
                        //serde, not required rn
                        let stamp_slice = remaining[0..stamp_end].trim();
                        let data: Vec<&str> = stamp_slice.split(" ").collect();
                        let lines_referenced = data.get(0);
                        if let Some(lines_num) = lines_referenced {
                            println!(
                                "This comment references the next {} lines of code",
                                lines_num
                            );
                            self.lines_of_code_referenced = lines_num.parse().expect("Could not parse the lines of code number of the ```comments-2.0 ``` stamp");
                        } else {
                            self.lines_of_code_referenced = 0;
                            println!("This comment needs the lines annotation");
                        }

                        let hash_of_lines = data.get(1);
                        if let Some(hash_str) = hash_of_lines {
                            println!("This comment is already stamped, the hash is: {}", hash_str);
                        } else {
                            println!("This comment has not been stamped");
                        }
                    }
                }
                None => {
                    self.raw_contents.push('\n');
                    self.raw_contents.push_str(string);
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
            //words, also certain characters should be ignored. ```comments-2.0 1```
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
        //words, also certain characters should be ignored. ```comments-2.0 1```
        pub fn hash_code(&self) -> String {
            //might need to implement a custom one at some point ```comments-2.0 1```
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

    pub struct SourceLocation {
        pub row: u64,
        //column will now not be used yet
        pub column: u64,
    }

    impl SourceLocation {
        pub fn empty() -> Self {
            Self { row: 0, column: 0 }
        }
    }

    pub struct SourceRange {
        pub start: SourceLocation,
        pub end: SourceLocation,
    }
}

#[cfg(test)]
mod tests {

    use crate::parser::parse_file;

    use super::*;
    use std::{fs, path};

    #[test]
    fn happy_path_single_line_comment() {
        let file_contents = "//this is a comment ```comments-2.0 1```
console.log(`hello world`);
";
        let result = parse_file(
            Path::new("file.js"),
            BufReader::new(file_contents.as_bytes()),
        );

        let all_have_file = result
            .iter()
            .all(|comments| !comments.file.to_str().unwrap().is_empty());
        assert!(all_have_file);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn happy_path_group_of_single_line_comments() {
        let file_contents = "//this is a group of single line comments
//that continues to the next line ```comments-2.0 1```
console.log(`hello world`);
";

        let result = parse_file(
            Path::new("happy.js"),
            BufReader::new(file_contents.as_bytes()),
        );

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
    fn can_detect_multiline_comments() {
        let file_contents = "/*
this is a multiline comment
that expands to multiple lines
```comments-2.0 1``` 
*/
console.log(`hello world`);
";

        let result = parse_file(
            Path::new("happy.js"),
            BufReader::new(file_contents.as_bytes()),
        );

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
//that should be considered one ```comments-2.0 2```
console.log(`Line 1`)
console.log(`Line 2`)

/* a multiline comment
that has many more lines
and it doesnt end really

```comments-2.0 2```
*/
console.log(`Line 3`)
console.log(`Line 4`)
";

        let result = parse_file(
            Path::new("happy.js"),
            BufReader::new(file_contents.as_bytes()),
        );

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
        let file_contents = "console.log(`Hello World`); //this prints hello world to the console";

        let result = parse_file(
            &Path::new("inline_comments.js"),
            BufReader::new(file_contents.as_bytes()),
        );

        assert!(result.len() == 0);
    }

    #[test]
    fn no_empty_comments() {
        let file_contents = "// single line comment ```comments-2.0 1```
console.log(`hello world`)

//group of single line comments
//that should be considered one ```comments-2.0 2```
console.log(`Line 1`)
console.log(`Line 2`)

/* a multiline comment
that has many more lines
and it doesnt end really

```comments-2.0 2```
*/
console.log(`Line 3`)
console.log(`Line 4`)

//
";

        let result = parse_file(
            Path::new("happy.js"),
            BufReader::new(file_contents.as_bytes()),
        );

        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|comment| comment.raw_contents.len() > 0));
    }

    #[test]
    fn ignores_comments_in_strings() {
        //this is very niche just for the bugs it might cause i will implement it
        todo!();
    }

    #[test]
    fn rejects_invalid_arguments() {
        //this can also be a test-doc how's it called in rust
        todo!();
    }

    #[test]
    fn partial_and_full_scan_produce_the_same_result() {
        //the partial scan will later be implemented, basically a project directory, a Vec<Diffs>,
        //a Sqlite database
        todo!();
    }
}
