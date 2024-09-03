//!
//! All roles used within application
//!

use strum::AsRefStr;

#[derive(AsRefStr)]
pub enum Role {
    #[strum(serialize = "tom_notifier_produce_notifications")]
    ProduceNotifications,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn produce_notifications() {
        let role = Role::ProduceNotifications.as_ref();
        assert_eq!(role, "tom_notifier_produce_notifications");
    }
}
