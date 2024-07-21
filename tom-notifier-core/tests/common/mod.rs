pub fn address() -> String {
    std::env::var("TOM_NOTIFIER_CORE_BIND_ADDRESS").unwrap()
}
