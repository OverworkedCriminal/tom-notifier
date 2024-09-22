#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("insert unique violation")]
    InsertUniqueViolation,

    #[error("no document updated")]
    NoDocumentUpdated,

    #[error("mongo error: {0}")]
    Mongo(#[from] mongodb::error::Error),
}
