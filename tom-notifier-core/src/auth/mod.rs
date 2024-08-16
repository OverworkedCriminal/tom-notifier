mod dto;
mod middleware;
mod roles;

pub use dto::User;
pub use middleware::jwt_auth_layer::JwtAuthLayer;
pub use roles::*;

use crate::error::Error;

///
/// Validates that user has all required roles.
///
/// ### Errors
/// - [Error::MissingRole] when any of the roles is missing
///
pub fn require_all_roles(user: &User, roles: &[Role]) -> Result<(), Error> {
    let user_has_all_roles = roles
        .into_iter()
        .map(|role| role.as_ref())
        .all(|role| user.roles.iter().any(|user_role| user_role == role));

    match user_has_all_roles {
        true => Ok(()),
        false => Err(Error::MissingRole),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn require_roles_user_has_role() {
        let user = User::new(
            Uuid::new_v4(),
            vec![
                "first_other_application_role".to_string(),
                Role::ProduceNotifications.as_ref().to_string(),
                "second_other_application_role".to_string(),
            ],
        );

        let result = require_all_roles(&user, &[Role::ProduceNotifications]);

        assert!(result.is_ok());
    }

    #[test]
    fn require_roles_user_does_not_have_role() {
        let user = User::new(
            Uuid::new_v4(),
            vec![
                "first_other_application_role".to_string(),
                "second_other_application_role".to_string(),
            ],
        );

        let result = require_all_roles(&user, &[Role::ProduceNotifications]);

        assert!(result.is_err());
    }
}
