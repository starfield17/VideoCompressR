use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum CoreError {
    #[error("{0}")]
    InvalidValue(String),
}
