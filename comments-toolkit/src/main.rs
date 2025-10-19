use std::collections::HashMap;
use std::env::Args;
use std::fs::{File, read_dir};
use std::io::BufReader;
use std::iter::Iterator;
use std::path::{Path, PathBuf};
use std::{env, panic, u64};

use storage::SqliteDB;
fn main() {
    use parser::Parser;
    use storage::Storage;
    let program_args = env::args();

    let options = parse_program_args(program_args)
        .inspect_err(|e| eprintln!("Error while parsing args: {e}"))
        .unwrap();

    assert!(options.len() > 0);

    match are_args_valid(&options) {
        Ok(()) => println!("Check: Arguments are valid"),
        Err(e) => eprintln!("Error while checking option combinations validity {e}"),
    }

    let project_files = get_files_from_directory_recursively(
        options
            .get("source")
            .expect("should provide --source flag")
            .into(),
        &vec!["target".into()],
        &vec!["rs".to_owned()],
    );

    println!("Will process {} project files", project_files.len());

    //O MANOS GAMIETAI ```comments-2.0 4 aslkjdahsdjkhasd```
    let comment_data_of_files: Vec<models::CommentData> = project_files
        .iter()
        .flat_map(|p| parser::parse_file(p, BufReader::new(File::open(p).unwrap())))
        .collect();

    println!("The project comments are: {}", comment_data_of_files.len());
}

fn get_files_from_directory_recursively(
    dir: PathBuf,
    ignored_dirs: &Vec<String>,
    file_extensions_allowed: &Vec<String>,
) -> Vec<PathBuf> {
    println!("The dir is: {:?}", dir);
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
    use rusqlite::Connection;
    use std::path::PathBuf;
    pub struct SqliteDB {
        file: PathBuf,
    }

    impl SqliteDB {
        pub fn new(file: PathBuf) -> Result<Self, rusqlite::Error> {
            let conn = Connection::open(&file)?;
            return Ok(Self { file });
        }
    }

    impl Storage for SqliteDB {
        fn store(&self, data: &models::CommentData) -> bool {
            return false;
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
        fn store(&self, data: &models::CommentData) -> bool {
            return false;
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

    use super::{models, storage};
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::prelude::*;
    use std::io::{self, BufReader};
    use std::path::{Path, PathBuf};

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

    //The parser assumes the options are all valid
    //we dont need ownership of the options
    pub struct Parser {
        options: HashMap<String, String>,
        state: Option<ParserState>,
        //if performance gets bad this might be changed to SQLite to avoid the vtable cost
        storage: Box<dyn storage::Storage>,
    }

    impl Parser {
        pub fn new(options: HashMap<String, String>, storage: Box<dyn storage::Storage>) -> Self {
            Parser {
                options,
                storage,
                state: None,
            }
        }

        pub fn debug_dump(&self) {
            println!("Options are {:?}", &self.options);
            println!("DB is sqlite");
        }
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

    fn get_files_in_directory(directory: &Path) -> Vec<&Path> {
        assert!(directory.is_dir());
        todo!()
    }

    //this does not belong here
    fn apply_integrity_rules(
        parsed_comments: Vec<Vec<(PathBuf, CommentData)>>,
    ) -> Result<(), String> {
        return Ok(());
    }

    pub fn parse_project_in(directory: &Path) -> Vec<Vec<(PathBuf, CommentData)>> {
        assert!(directory.is_dir());
        //this should call parse_file
        return Vec::new();
    }

    pub fn parse_file<T: BufRead>(file: &Path, reader: T) -> Vec<CommentData> {
        // let f = File::open(file);
        // assert!(f.is_ok());
        // let reader = BufReader::new(f.unwrap());
        let mut parser_state = ParserState::empty();
        let mut current_comment_data = CommentData::empty();
        let mut result = Vec::new();
        current_comment_data.file = file.into();
        let mut comments_number = 0;
        for l in reader.lines() {
            //this should be extracted to a different function
            if let Ok(current_line) = l {
                println!("Current line: {current_line}\n");
                let current_line = current_line.trim_start();
                if current_line.starts_with("/*") {
                    comments_number += 1;
                    parser_state.set_state(ParserPositionType::IN_MULTILINE_COMMENT);
                    let comment: String = current_line.to_string().chars().skip(2).collect();
                    current_comment_data.push_comment(&comment);
                    println!("Entering multiline comment\n");
                } else if current_line.starts_with("//") {
                    comments_number += 1;
                    println!("Entering single line comment\n");
                    let comment: String = current_line.to_string().chars().skip(2).collect();
                    current_comment_data.push_comment(&comment);
                    parser_state.set_state(ParserPositionType::IN_SINGLE_LINE_COMMENT);
                } else if current_line.starts_with("*/") {
                    println!("Exiting multiline comment\n");
                    parser_state.set_state(ParserPositionType::NONE);
                } else {
                    if parser_state.position == ParserPositionType::IN_MULTILINE_COMMENT {
                        comments_number += 1;
                        current_comment_data.push_comment(current_line.trim_start());
                        continue;
                    }

                    if parser_state.position != ParserPositionType::NOT_IN_A_COMMENT {
                        parser_state.set_state(ParserPositionType::NOT_IN_A_COMMENT);
                    }

                    if parser_state.previous == ParserPositionType::IN_SINGLE_LINE_COMMENT
                        || parser_state.previous == ParserPositionType::IN_MULTILINE_COMMENT
                    {
                        //means it is unstamped as we are currently in a line of code
                        //```comments-2.0 1```
                        if current_comment_data.lines_of_code_referenced == 0 {
                            println!("Pushing a comment to the result\n");
                            result.push(current_comment_data);
                            current_comment_data = CommentData::empty();
                            parser_state.set_state(ParserPositionType::NOT_IN_A_COMMENT);
                            continue;
                        }
                    }

                    println!("Entering a line of code\n");
                    if parser_state.previous == ParserPositionType::IN_SINGLE_LINE_COMMENT
                        || parser_state.previous == ParserPositionType::IN_MULTILINE_COMMENT
                    {
                        //this means it is stamped ```comments-2.0 3```
                        if current_comment_data.lines_of_code_referenced > 0
                            && current_comment_data.lines_of_code_read
                                == current_comment_data.lines_of_code_referenced
                        {
                            //Hello World
                            //This is the first comment of the app ```comments-2.0 3```
                            println!("Pushing a comment to the result\n");
                            result.push(current_comment_data);
                            current_comment_data = CommentData::empty();
                        } else if current_comment_data.lines_of_code_referenced > 0
                            && current_comment_data.lines_of_code_read
                                != current_comment_data.lines_of_code_referenced
                        {
                            println!("Pushing code to existing comment");
                            current_comment_data.lines_of_code_read += 1;
                            current_comment_data.push_code(current_line.trim_start());
                        }
                    }
                }
            } else {
                break;
            }
        }
        //when it finishes we just pick up any relevant comment
        if current_comment_data.raw_contents.len() > 0 {
            result.push(current_comment_data);
        }

        println!("Lines of comments are: {comments_number}");
        println!("Comments found: {}", result.len());
        return result;
    }
}
pub mod models {
    use std::{hash::Hash, path::PathBuf};

    // i need a different struct for the parser and the db

    pub struct CommentData {
        pub location: SourceRange,
        //used in the generation of the hash
        pub raw_contents: String,
        pub code_it_refers_to: String,
        pub hash_from_comment: String,

        //most of the fields should be optional, to signal when they are unstamped
        pub lines_of_code_referenced: u16,
        pub lines_of_code_read: u16,
        pub file: PathBuf,
        pub computed_hash: String,
    }

    impl CommentData {
        pub fn empty() -> Self {
            Self {
                location: SourceRange {
                    start: SourceLocation { row: 0, column: 0 },
                    end: SourceLocation { row: 0, column: 0 },
                },
                raw_contents: "".into(),
                computed_hash: "".into(),
                file: PathBuf::default(),
                code_it_refers_to: "".into(),
                hash_from_comment: "".into(),
                lines_of_code_referenced: 0,
                lines_of_code_read: 0,
            }
        }

        pub fn push_comment(&mut self, string: &str) {
            //should ignore the part of the string that
            //has the comments-2.0 stamp
            //and also parse that part to see how many lines of code should be parsed next
            let final_str = "";
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

    impl Hash for CommentData {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            //using only the code contents and the filename, also the content should first be split into
            //words, also certain characters should be ignored. ```comments-2.0 1```
            todo!()
        }
    }

    pub struct SourceLocation {
        row: u64,
        //column will now not be used yet
        column: u64,
    }

    impl SourceLocation {
        pub fn empty() -> Self {
            Self { row: 0, column: 0 }
        }
    }

    pub struct Settings {
        //all flags i've mentioned in the doc go here
    }

    pub struct SourceRange {
        start: SourceLocation,
        end: SourceLocation,
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
            assert!(has_code || has_comment);
        }
    }

    #[test]
    fn ignores_inline_comments() {
        let file_contents = "console.log(`Hello World`); //this prints hello world to the console";

        let temp_file_path = Path::new(&std::env::temp_dir()).join("happy_path_inline_comments.js");
        let _ = fs::write(&temp_file_path, &file_contents);
        let result = parse_file(&temp_file_path, BufReader::new(file_contents.as_bytes()));
        let _ = fs::remove_file(temp_file_path);

        assert!(result.len() == 0);
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
