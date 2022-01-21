#[derive(thiserror::Error, Debug)]
pub enum JournalError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] rustbreak::error::RustbreakError),
}