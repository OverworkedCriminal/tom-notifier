use strum::AsRefStr;

#[derive(AsRefStr)]
pub enum Role {
    #[strum(serialize = "tom_notifier_admin")]
    Admin,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn admin() {
        let role = Role::Admin.as_ref();
        assert_eq!(role, "tom_notifier_admin");
    }
}
