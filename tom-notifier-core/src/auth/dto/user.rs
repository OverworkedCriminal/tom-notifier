use std::{ops::Deref, sync::Arc};
use uuid::Uuid;

///
/// Struct with user information.
///
/// To make sure cloning does not take too long
/// all fields are stored in InnerUser behind an Arc.
///
/// InnerUser fields are accessible thanks to Deref trait.
///
#[derive(Clone)]
pub struct User {
    inner: Arc<InnerUser>,
}

///
/// User information retrieved from his JWT.
///
pub struct InnerUser {
    id: Uuid,
    roles: Vec<String>,
}

impl User {
    pub fn new(id: Uuid, roles: Vec<String>) -> Self {
        Self {
            inner: Arc::new(InnerUser { id, roles }),
        }
    }
}

impl Deref for User {
    type Target = InnerUser;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
