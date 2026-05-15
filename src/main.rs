#![forbid(unsafe_code)]

fn main() {
    if let Err(error) = obsidian_kb::run() {
        eprintln!("error: {error:?}");
        std::process::exit(1);
    }
}
