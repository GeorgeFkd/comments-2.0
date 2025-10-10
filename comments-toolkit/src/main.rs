use std::collections::HashMap;
use std::env::Args;
use std::iter::Iterator;
use std::{env, u64};
fn main() {
    let program_args = env::args();

    println!("Hello, world!, this is something new");
}

fn parse_program_args(args: Args) -> HashMap<String, String> {
    //the format is: --<argname1><space><value><space>--<argname2>
    return HashMap::new();
}

fn are_args_valid(args: &HashMap<String, String>) -> bool {
    return true;
}

mod storage {
    use super::*;
    use std::path::PathBuf;
    struct SqliteDB {
        file: PathBuf,
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
    use super::{models, storage};
    use std::collections::HashMap;
    use std::iter::Iterator;
    use std::path::Path;
    enum ParserPositionType {
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
    struct Parser {
        options: HashMap<String, String>,
        state: ParserState,
        //if performance gets bad this might be changed to SQLite to avoid the vtable cost
        storage: Box<dyn storage::Storage>,
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

    fn parse_project_in(directory: &Path) -> bool {
        assert!(directory.is_dir());
        //this should call parse_file
        return false;
    }

    fn parse_file(file: &Path) -> bool {
        return false;
    }
}
pub mod models {
    use std::{hash::Hash, path::PathBuf};

    pub struct CommentData {
        location: SourceRange,
        //used in the generation of the hash
        raw_contents: String,
        file: PathBuf,
        hash: String,
    }

    impl Hash for CommentData {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            todo!()
            //using only the contents and the filename, also the content should first be split into
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
