pub fn address() -> String {
    std::env::var("TOM_NOTIFIER_WS_DELIVERY_BIND_ADDRESS").unwrap()
}