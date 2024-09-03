#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Close(&'static str),

    #[error("{0}")]
    Anyhow(#[from] anyhow::Error),
}
