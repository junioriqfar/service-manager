#[path = "../diagnose.rs"]
mod diagnose;

fn main() {
    std::process::exit(diagnose::run());
}
