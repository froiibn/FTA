//! Keep startup errors visible while the reusable engine owns all runtime behavior.
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "fta-engine serve\nDirectories: FTA_CONFIG_DIR, FTA_STATE_DIR, FTA_RUNTIME_DIR, FTA_RECEIVE_DIR"
        );
        return;
    }
    if args.iter().any(|a| a != "serve") {
        eprintln!("unsupported argument; use fta-engine serve");
        std::process::exit(2);
    }
    let result = async {
        let paths = fta_core::config::AppPaths::discover()?;
        fta_core::engine::run(paths).await
    }
    .await;
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
