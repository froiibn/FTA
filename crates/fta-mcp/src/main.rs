//! Keep stdout exclusively for MCP; startup errors and process diagnostics belong on stderr.

use clap::Parser;
use std::process::ExitCode;
use std::time::Duration;

const RUNTIME_SHUTDOWN_MILLIS: u64 = 500;

fn main() -> ExitCode {
    let args = fta_mcp::Args::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("cannot start MCP runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(fta_mcp::run_stdio(args));
    // macOS/Windows stdio uses blocking reads; a client signal must not wait forever for stdin EOF.
    runtime.shutdown_timeout(Duration::from_millis(RUNTIME_SHUTDOWN_MILLIS));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FTA MCP stopped: {}", error.code);
            ExitCode::FAILURE
        }
    }
}
