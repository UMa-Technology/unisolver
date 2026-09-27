//! `cargo xtask <command>`: repository maintenance. See docs/upstream.md.
fn main() {
    eprintln!("usage: cargo xtask upstream <check|edit|export|sync>");
    std::process::exit(2);
}
