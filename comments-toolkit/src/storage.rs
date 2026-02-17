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
