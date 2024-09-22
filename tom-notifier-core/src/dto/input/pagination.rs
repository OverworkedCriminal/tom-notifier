use serde::Deserialize;

#[derive(Deserialize)]
pub struct Pagination {
    ///
    /// indexing starts at 0
    ///
    pub page_idx: u32,
    pub page_size: u32,
}
