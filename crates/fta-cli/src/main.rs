//! Keep process exits here so command behavior remains testable in the library.

#[tokio::main]
async fn main() {
    std::process::exit(fta_cli::main_entry().await);
}
