//! CLI requests go through the same private IPC as the desktop, preventing divergent state.

mod service;

use clap::{Parser, Subcommand};
use fta_core::{config::AppPaths, ipc};
use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const DEFAULT_INBOX_LIMIT: usize = 20;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const EXIT_INVALID: i32 = 2;
const EXIT_LOCAL_SERVICE: i32 = 8;
const EXIT_INTERRUPTED: i32 = 130;

/// Path overrides keep tests and multiple explicit instances out of personal app data.
#[derive(Debug, Parser)]
#[command(
    name = "fta",
    version,
    about = "Transfer files, text and URLs between your local devices"
)]
pub struct Args {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub config_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    pub state_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    pub runtime_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

/// Commands are explicit so listing or diagnosis never performs hidden writes.
#[derive(Debug, Subcommand)]
pub enum Command {
    Status,
    Devices,
    Pair {
        #[command(subcommand)]
        command: PairCommand,
    },
    Unpair {
        device_id: String,
    },
    Send {
        #[arg(long)]
        to: String,
        #[arg(required = true, num_args = 1..)]
        paths: Vec<PathBuf>,
    },
    Text {
        #[arg(long)]
        to: String,
        #[arg(conflicts_with = "stdin", required_unless_present = "stdin")]
        body: Option<String>,
        #[arg(long, conflicts_with = "body")]
        stdin: bool,
    },
    Url {
        #[arg(long)]
        to: String,
        url: String,
    },
    Transfer {
        #[command(subcommand)]
        command: TransferCommand,
    },
    Inbox {
        #[arg(long, default_value_t = DEFAULT_INBOX_LIMIT)]
        limit: usize,
        #[command(subcommand)]
        command: Option<InboxCommand>,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

/// Pairing is separate from automatic receipt: only the first trust setup needs confirmation.
#[derive(Debug, Subcommand)]
pub enum PairCommand {
    Open,
    /// The listening peer receives its comparison value only after the other device connects.
    Status {
        session_id: Option<String>,
    },
    Connect {
        target: String,
    },
    Confirm {
        session_id: String,
    },
    Cancel {
        session_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum TransferCommand {
    Show { transfer_id: String },
    Cancel { transfer_id: String },
}

#[derive(Debug, Subcommand)]
pub enum InboxCommand {
    Show {
        transfer_id: String,
        /// Raw content is only written to an explicitly requested file, never a terminal.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Delete {
        transfer_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Show,
    Set { key: String, value: String },
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    Install {
        #[arg(long)]
        engine: Option<PathBuf>,
    },
    Start,
    Stop,
    Status,
}

/// Structured failures preserve exit-code meaning without leaking message bodies to diagnostics.
#[derive(Debug)]
pub struct CliError {
    pub code: String,
    pub message: String,
    pub exit_code: i32,
}

impl CliError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_REQUEST".into(),
            message: message.into(),
            exit_code: EXIT_INVALID,
        }
    }

    fn service(message: impl Into<String>) -> Self {
        Self {
            code: "SERVICE_UNAVAILABLE".into(),
            message: message.into(),
            exit_code: EXIT_LOCAL_SERVICE,
        }
    }
}

/// Parse separately from execution to support help without discovering or creating app directories.
pub async fn main_entry() -> i32 {
    let raw_args: Vec<_> = std::env::args_os().collect();
    let wants_json = raw_args.iter().any(|argument| argument == "--json");
    let args = match Args::try_parse_from(raw_args) {
        Ok(args) => args,
        Err(error) => {
            let code = if error.use_stderr() { EXIT_INVALID } else { 0 };
            // Scripts need a parse failure in the same JSON envelope as runtime failures.
            if wants_json && error.use_stderr() {
                let output = json!({"ok":false,"error":{"code":"INVALID_REQUEST","message":error.to_string()}});
                if writeln!(io::stdout().lock(), "{output}").is_err() {
                    return EXIT_LOCAL_SERVICE;
                }
                return code;
            }
            if let Err(write_error) = error.print() {
                eprintln!("could not write command help: {write_error}");
                return EXIT_LOCAL_SERVICE;
            }
            return code;
        }
    };
    let json_output = args.json;
    match execute(args).await {
        Ok(result) => {
            let exit_code = result_exit_code(&result);
            let output = if json_output {
                serde_json::to_string(&json!({"ok": exit_code == 0, "result": result}))
            } else {
                serde_json::to_string_pretty(&result)
            };
            match output {
                Ok(output) => match writeln!(io::stdout().lock(), "{output}") {
                    Ok(()) => exit_code,
                    Err(_) => EXIT_LOCAL_SERVICE,
                },
                Err(_) => EXIT_LOCAL_SERVICE,
            }
        }
        Err(error) => {
            if json_output {
                let output =
                    json!({"ok": false, "error": {"code": error.code, "message": error.message}});
                if writeln!(io::stdout().lock(), "{output}").is_err() {
                    return EXIT_LOCAL_SERVICE;
                }
            } else {
                eprintln!("{}: {}", error.code, terminal_safe(&error.message));
            }
            error.exit_code
        }
    }
}

/// Keep all user-visible state mutations in the engine; service control is the sole OS exception.
pub async fn execute(args: Args) -> Result<Value, CliError> {
    let mut paths = AppPaths::discover().map_err(|error| CliError::invalid(error.to_string()))?;
    if let Some(path) = args.config_dir {
        paths.config_dir = absolute(&path)?;
    }
    if let Some(path) = args.state_dir {
        paths.state_dir = absolute(&path)?;
    }
    if let Some(path) = args.runtime_dir {
        paths.runtime_dir = absolute(&path)?;
    }

    if let Command::Service { command } = &args.command {
        return service::execute(command, &paths);
    }
    let output_path = match &args.command {
        Command::Inbox {
            command: Some(InboxCommand::Show { output, .. }),
            ..
        } => output.clone(),
        _ => None,
    };
    let (method, params) = request_for(&args.command, &mut io::stdin().lock())?;
    let transfer_id = params
        .get("transfer_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if method == "transfers.send"
        && let Some(id) = &transfer_id
    {
        eprintln!("transfer_id: {id}");
    }

    // Interrupting a socket alone would leave remote receipt ambiguous, so request cancellation explicitly.
    let response = if method == "transfers.send" {
        tokio::select! {
            result = ipc::request(&paths, method, params) => result.map_err(engine_error)?,
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| CliError::service(error.to_string()))?;
                if let Some(id) = transfer_id {
                    let cancellation = ipc::request(&paths, "transfers.cancel", json!({"transfer_id":id})).await
                        .map_err(|error| CliError::service(format!("interrupt received but cancellation was not confirmed: {error}")))?;
                    let mut result = cancellation;
                    if let Some(object) = result.as_object_mut() { object.insert("interrupted".into(), Value::Bool(true)); }
                    result
                } else {
                    return Err(CliError { code: "INTERRUPTED".into(), message: "interrupted before a transfer was submitted".into(), exit_code: EXIT_INTERRUPTED });
                }
            }
        }
    } else {
        ipc::request(&paths, method, params)
            .await
            .map_err(engine_error)?
    };
    // IPC failures are already Err; a transfer's nullable error field is record data, not an envelope.
    // Preserve the full record and let its terminal state determine the command's exit status.
    if let Some(output) = output_path {
        let body = response
            .get("body")
            .or_else(|| response.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| CliError::invalid("this inbox item has no text body"))?;
        // Explicit raw export must not silently overwrite a user's existing file.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            // Message exports may contain private text, so do not inherit a permissive umask.
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&output)
            .map_err(|error| CliError::invalid(format!("cannot create output file: {error}")))?;
        file.write_all(body.as_bytes())
            .map_err(|error| CliError::invalid(format!("cannot write output file: {error}")))?;
        file.sync_all()
            .map_err(|error| CliError::invalid(format!("cannot flush output file: {error}")))?;
        return Ok(json!({"written": output}));
    }
    // JSON serialization escapes terminal control characters even when the sender supplied arbitrary text.
    Ok(response)
}

/// This contract is testable without an engine and is shared by every Linux command path.
pub fn request_for(
    command: &Command,
    input: &mut impl Read,
) -> Result<(&'static str, Value), CliError> {
    match command {
        Command::Status => Ok(("status", json!({}))),
        Command::Devices => Ok(("devices.list", json!({}))),
        Command::Pair { command } => match command {
            PairCommand::Open => Ok(("pairing.open", json!({}))),
            PairCommand::Status { session_id } => {
                Ok(("pairing.status", json!({"session_id":session_id})))
            }
            PairCommand::Connect { target } => Ok(("pairing.connect", json!({"target":target}))),
            PairCommand::Confirm { session_id } => {
                Ok(("pairing.confirm", json!({"session_id":session_id})))
            }
            PairCommand::Cancel { session_id } => {
                Ok(("pairing.cancel", json!({"session_id":session_id})))
            }
        },
        Command::Unpair { device_id } => Ok(("devices.unpair", json!({"device_id":device_id}))),
        Command::Send { to, paths } => {
            let paths = paths.iter().map(|path| {
                let path = absolute(path)?;
                let metadata = std::fs::symlink_metadata(&path).map_err(|error| CliError::invalid(format!("cannot inspect input file: {error}")))?;
                if !metadata.file_type().is_file() { return Err(CliError::invalid("only regular files may be sent; directories and symbolic links are unsupported")); }
                Ok(path)
            }).collect::<Result<Vec<_>, CliError>>()?;
            Ok((
                "transfers.send",
                json!({"to":to,"kind":"files","paths":paths,"transfer_id":uuid::Uuid::new_v4()}),
            ))
        }
        Command::Text { to, body, stdin } => {
            let body = if *stdin {
                let mut bytes = Vec::new();
                input
                    .take((MAX_MESSAGE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|error| CliError::invalid(format!("cannot read stdin: {error}")))?;
                String::from_utf8(bytes)
                    .map_err(|_| CliError::invalid("stdin must contain UTF-8 text"))?
            } else {
                body.clone()
                    .ok_or_else(|| CliError::invalid("text or --stdin is required"))?
            };
            if body.len() > MAX_MESSAGE_BYTES {
                return Err(CliError::invalid("text exceeds the 1 MiB limit"));
            }
            Ok((
                "transfers.send",
                json!({"to":to,"kind":"text","body":body,"transfer_id":uuid::Uuid::new_v4()}),
            ))
        }
        Command::Url { to, url } => {
            if url.len() > MAX_MESSAGE_BYTES {
                return Err(CliError::invalid("URL exceeds the 1 MiB limit"));
            }
            Ok((
                "transfers.send",
                json!({"to":to,"kind":"url","body":url,"transfer_id":uuid::Uuid::new_v4()}),
            ))
        }
        Command::Transfer { command } => match command {
            TransferCommand::Show { transfer_id } => {
                Ok(("transfers.get", json!({"transfer_id":transfer_id})))
            }
            TransferCommand::Cancel { transfer_id } => {
                Ok(("transfers.cancel", json!({"transfer_id":transfer_id})))
            }
        },
        Command::Inbox { limit, command } => match command {
            None if *limit > 0 => Ok(("inbox.list", json!({"limit":limit}))),
            None => Err(CliError::invalid("--limit must be greater than zero")),
            Some(InboxCommand::Show { transfer_id, .. }) => {
                Ok(("inbox.get", json!({"transfer_id":transfer_id})))
            }
            Some(InboxCommand::Delete { transfer_id }) => {
                Ok(("inbox.delete", json!({"transfer_id":transfer_id})))
            }
        },
        Command::Config { command } => match command {
            ConfigCommand::Show => Ok(("settings.get", json!({}))),
            ConfigCommand::Set { key, value } => {
                // CLI spelling is ergonomic, while the engine receives one canonical typed contract.
                let key = key.replace('-', "_");
                let value = match key.as_str() {
                    "display_name" | "listen_addr" => Value::String(value.clone()),
                    "receive_dir" => serde_json::to_value(absolute(Path::new(value))?)
                        .map_err(|error| CliError::invalid(error.to_string()))?,
                    "discovery_enabled" => Value::Bool(value.parse::<bool>().map_err(|_| {
                        CliError::invalid("discovery-enabled must be true or false")
                    })?),
                    "allowed_networks" => {
                        let networks: Vec<String> = serde_json::from_str(value).map_err(|_| {
                            CliError::invalid(
                                "allowed-networks must be a JSON array of CIDR strings",
                            )
                        })?;
                        json!(networks)
                    }
                    _ => {
                        return Err(CliError::invalid(
                            "unsupported setting; use config show to inspect supported keys",
                        ));
                    }
                };
                Ok(("settings.update", json!({"key":key,"value":value})))
            }
        },
        Command::Service { .. } => Err(CliError::invalid(
            "service commands must use the platform service adapter",
        )),
    }
}

/// JSON escaping handles regular output; stderr also needs explicit control-byte neutralization.
pub fn terminal_safe(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() && character != '\n' && character != '\t' {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn absolute(path: &Path) -> Result<PathBuf, CliError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|base| base.join(path))
            .map_err(|error| CliError::invalid(error.to_string()))
    }
}

fn remote_error(error: &Value) -> CliError {
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("REMOTE_ERROR");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("engine request failed");
    let exit_code = match code {
        "INVALID_REQUEST" | "TOO_LARGE" | "CONFIG_ERROR" => EXIT_INVALID,
        "OFFLINE" | "TIMEOUT" | "BUSY" => 3,
        // Missing registration discovery is a pairing failure, not a broken local IPC service.
        "UNTRUSTED" | "KEY_CHANGED" | "PAIRING_NOT_FOUND" => 4,
        "NO_SPACE" | "PERMISSION_DENIED" | "SOURCE_CHANGED" | "HASH_MISMATCH" => 5,
        "UNKNOWN" | "RESULT_UNKNOWN" => 7,
        "CANCELLED" => EXIT_INTERRUPTED,
        _ => EXIT_LOCAL_SERVICE,
    };
    CliError {
        code: code.into(),
        message: message.into(),
        exit_code,
    }
}

fn engine_error(error: fta_core::Error) -> CliError {
    remote_error(&json!({"code":error.code,"message":error.message}))
}

/// Exit status follows the final receiver result, not merely successful local IPC delivery.
pub fn result_exit_code(result: &Value) -> i32 {
    match result
        .get("state")
        .or_else(|| result.get("status"))
        .and_then(Value::as_str)
    {
        Some("partial") => 6,
        Some("unknown") => 7,
        Some("failed") => 5,
        Some("cancelled") => EXIT_INTERRUPTED,
        _ if result.get("interrupted").and_then(Value::as_bool) == Some(true) => EXIT_INTERRUPTED,
        _ => 0,
    }
}
