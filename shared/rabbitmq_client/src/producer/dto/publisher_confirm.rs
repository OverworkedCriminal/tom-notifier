use super::publisher_confirm_variant::PublisherConfirmVariant;

pub struct PublisherConfirm {
    pub delivery_tag: u64,
    pub multiple: bool,
    pub variant: PublisherConfirmVariant,
}
