use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("tetra3: {0}")]
    Tetra3(#[from] tetra3::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("missing data: {0}")]
    MissingData(String),
}
pub type Result<T> = std::result::Result<T, CoreError>;
