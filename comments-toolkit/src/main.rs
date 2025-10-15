use std::collections::HashMap;
use std::env::Args;
use std::fs::read_dir;
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
        &vec![
            "assets".to_owned(),
            "build".to_owned(),
            "cmake-build-debug".to_owned(),
        ],
        &vec!["h".to_owned(), "cpp".to_owned()],
    );

    println!("The project files are: {}", project_files.len());

    println!("Hello, world!, this is something new");
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
    use crate::models::CommentData;

    use super::{models, storage};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    //we will be ignoring the inline comments probably
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
        position: ParserPositionType,
        location: models::SourceLocation,
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

    fn parse_project_in(directory: &Path) -> Vec<Vec<(PathBuf, CommentData)>> {
        assert!(directory.is_dir());
        //this should call parse_file
        return Vec::new();
    }

    fn parse_file(file: &Path) -> Vec<CommentData> {
        return Vec::new();
    }
}
pub mod models {
    use std::{hash::Hash, path::PathBuf};

    pub enum CommentReference {
        WholeFile(PathBuf),
        Code(String),
    }

    pub struct CommentData {
        location: SourceRange,
        //used in the generation of the hash
        raw_contents: String,
        refers_to: CommentReference,
        file: PathBuf,
        hash: String,
    }

    impl Hash for CommentData {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            todo!()
            //using only the code contents and the filename, also the content should first be split into
            //words, also certain characters should be ignored.
        }
    }

    pub struct SourceLocation {
        row: u64,
        column: u64,
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
    use super::*;
    #[test]
    fn multiple_line_comments_are_seen_as_group() {
        todo!()
    }

    #[test]
    fn can_detect_multiline_comments() {
        todo!()
    }
    #[test]
    fn ignores_inline_comments() {
        todo!()
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
        //
        todo!();
    }
}
