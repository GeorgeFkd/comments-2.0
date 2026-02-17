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
