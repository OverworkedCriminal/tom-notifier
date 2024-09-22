#[derive(Debug, thiserror::Error)]
#[error("missing role: {missing_role}")]
pub struct MissingRoleError {
    pub missing_role: String,
}
