pub fn version() -> &'static str {
    option_env!("OBSIDIAN_KB_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}
