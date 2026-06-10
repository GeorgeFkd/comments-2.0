use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};
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
    pub dependency_list_parsed: Vec<String>,
    pub parse_error: Option<StampParseError>,
}

impl<'a> CommentData<'a> {
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
            dependency_list_parsed: vec![],
        }
    }

    fn consume_stamp(&mut self, stamp_contents: &str) -> () {
        let data: Vec<&str> = stamp_contents.split(" ").collect();
        //TODO: First the user specified stuff and then the rest
        //TODO: A comment not having hashes is not an error specifically it is just that it hasnt
        //hashes generated yet.
        let parse_error = match data.len() {
            0 => Some(StampParseError::StampWithoutLinesReferenced),
            1 => Some(StampParseError::StampWithoutHashes),
            2 => Some(StampParseError::StampWithoutCodeHash),
            _ => None,
        };
        self.parse_error = parse_error.clone();

        let lines_referenced = data.get(0).unwrap();
        let parsed_num = lines_referenced.parse().expect(&format!(
            "Could not parse the lines of code number of the ```comments-2.0 ``` stamp at {}:{}:{}",
            self.file.display(),
            self.comment_location.start.row,
            self.comment_location.start.column
        ));
        self.lines_of_code_referenced = parsed_num;
        //user wants us to ignore this comment ```comments-2.0 3 427109853614679882 427109853614679882```
        if parsed_num == 0 {
            self.should_be_ignored = true;
        }
        //we have the stamp with lines <comma_separated_deps_list> <comment_hash> <code_hash> ```comments-2.0 1 1687416639044599693 1687416639044599693```
        let mut position_of_code_hash = 0;
        let mut position_of_comment_hash = 0;
        let mut position_of_comment_deps = 0;
        if data.len() == 4 {
            position_of_comment_deps = 1;
            position_of_comment_hash = 2;
            position_of_code_hash = 3;
            println!("User has included dependencies in comment");
        } else if data.len() == 3 {
            position_of_comment_hash = 1;
            position_of_code_hash = 2;
        }
        assert!(
            self.parse_error.is_some()
                || (position_of_code_hash != 0 && position_of_comment_hash != 0)
        );

        if parse_error.is_some() {
            return;
        }
        // println!("Extra data is: {:?}", data);
        // println!(
        //     "This comment references the next {} lines of code",
        //     lines_referenced
        // );

        let hash_of_comment = data.get(position_of_comment_hash).unwrap();
        self.comment_hash_parsed = hash_of_comment.trim().to_string();
        // println!(
        //     "This comment is already stamped with comment hash, the hash is: {}",
        //     hash_of_comment
        // );

        let hash_of_code = data.get(position_of_code_hash).unwrap();
        self.code_hash_parsed = hash_of_code.trim().to_string();
        // println!(
        //     "This comment is already stamped with code hash, the hash is: {}",
        //     hash_of_code
        // );
        if position_of_comment_deps != 0 {
            // If this is slow the parse_num_list is at fault: ```comments-2.0 5 8442626637817291417, 17128342224092336620 17128342224092336620```
            let dependency_list_string = data.get(position_of_comment_deps);
            self.dependency_list_parsed = match dependency_list_string {
                None => vec![],
                Some(str) => parse_num_list_from_str(str),
            };
            // println!("Dependency list now is: {:?}", self.dependency_list_parsed);
        }

        return;
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
                self.consume_stamp(stamp_slice);
                // println!("Stamp slice is: {:?}", stamp_slice);

                return Some(start + open_pattern.len() + stamp_end.unwrap());
            }
        }
    }

    pub fn push_code(&mut self, string: &str) {
        self.code_it_refers_to.push_str(string);
        self.code_it_refers_to.push('\n');
    }
}

fn parse_num_list_from_str(str: &str) -> Vec<String> {
    //21312439012,124322342131,1423314312321
    //This is costly ```comments-2.0 1 8442626637817291417 8442626637817291417```
    let num_list: Vec<String> = str
        .split(",")
        .map(|n| n.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    return num_list;
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

#[cfg(test)]
mod tests {
    use crate::models::*;

    fn empty_comment<'a>() -> CommentData<'a> {
        CommentData::empty()
    }

    #[test]
    fn comment_with_normal_dependencies() {
        let mut cm = empty_comment();
        let _ = cm.push_comment("This is a comment with dependencies ```comments-2.0 1 21332423412312,123123123123 04935783498753948 12349034901840398 ```");
        cm.push_code("console.log(`Hello from dependencied comment`)");
        assert!(cm.parse_error.is_none());
        assert_eq!(cm.lines_of_code_referenced, 1);
        assert_eq!(cm.comment_hash_parsed, "04935783498753948");
        assert_eq!(cm.code_hash_parsed, "12349034901840398");
        assert_eq!(
            cm.dependency_list_parsed,
            vec!["21332423412312", "123123123123"]
        )
    }

    #[test]
    fn comment_with_missing_comma_on_single_dependency() {
        let mut cm = empty_comment();
        let _ = cm.push_comment("This is a comment with dependencies ```comments-2.0 1 21332423412312 04935783498753948 12349034901840398 ```");
        cm.push_code("console.log(`Hello from dependencied comment`)");
        assert!(cm.parse_error.is_none());
        assert_eq!(cm.lines_of_code_referenced, 1);
        assert_eq!(cm.comment_hash_parsed, "04935783498753948");
        assert_eq!(cm.code_hash_parsed, "12349034901840398");
        assert_eq!(cm.dependency_list_parsed, vec!["21332423412312"]);
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

