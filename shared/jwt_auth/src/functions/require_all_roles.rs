use crate::{error::MissingRoleError, User};

///
/// Validates that user has all required roles.
///
/// ### Errors
/// - [MissingRoleError] when any of the roles is missing
///
pub fn require_all_roles(user: &User, roles: &[&str]) -> Result<(), MissingRoleError> {
    for role in roles {
        let found_role = user.roles.iter().any(|user_role| user_role == role);
        if !found_role {
            return Err(MissingRoleError {
                missing_role: role.to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn require_roles_user_has_role() {
        let required_role = "required_role_for_application";

        let user = User::new(
            Uuid::from_u128(1059812038128571928731),
            vec![
                "first_other_application_role".to_string(),
                required_role.to_string(),
                "second_other_application_role".to_string(),
            ],
        );

        let result = require_all_roles(&user, &[required_role]);

        assert!(result.is_ok());
    }

    #[test]
    fn require_roles_user_does_not_have_role() {
        let missing_role = "this_role_should_not_be_in_users_possession";

        let user = User::new(
            Uuid::from_u128(1059812038128571928731),
            vec![
                "first_other_application_role".to_string(),
                "second_other_application_role".to_string(),
            ],
        );

        let result = require_all_roles(&user, &[missing_role]);

        assert!(result.is_err());
    }
}
