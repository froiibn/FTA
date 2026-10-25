//! Length-delimited local control bounds memory use and avoids publishing a TCP admin endpoint.
use crate::{
    Error, Result,
    config::{AppPaths, MAX_CONTROL_BYTES},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub request_id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}
#[derive(Serialize, Deserialize)]
struct Response {
    request_id: String,
    result: Option<Value>,
    error: Option<Error>,
}
/// Await the actual engine result; an IPC connection alone is never reported as success.
pub async fn request(paths: &AppPaths, method: &str, params: Value) -> Result<Value> {
    #[cfg(unix)]
    let mut socket = tokio::net::UnixStream::connect(paths.socket_path())
        .await
        .map_err(|e| Error::new("SERVICE_UNAVAILABLE", e.to_string()))?;
    #[cfg(windows)]
    let mut socket = connect_windows_pipe(
        &paths.pipe_name()?,
        std::time::Duration::from_secs(crate::config::CONNECT_SECONDS),
    )
    .await?;
    let request = Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: method.into(),
        params,
    };
    write_frame(&mut socket, &serde_json::to_vec(&request)?).await?;
    // A large video may legitimately take hours; only inbound frame bytes have an idle limit.
    let response: Response = serde_json::from_slice(&read_response(&mut socket).await?)?;
    if response.request_id != request.request_id {
        return Err(Error::new("IPC_ERROR", "response ID mismatch"));
    }
    match (response.result, response.error) {
        (Some(v), None) => Ok(v),
        (_, Some(e)) => Err(e),
        _ => Err(Error::new("IPC_ERROR", "invalid response envelope")),
    }
}

#[cfg(windows)]
async fn connect_windows_pipe(
    name: &str,
    budget: std::time::Duration,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
    const RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(name) {
            Ok(client) => return Ok(client),
            // The engine replaces its listening instance after each connection. Concurrent
            // views can hit that short busy window, which does not mean the engine is down.
            // Retry only before a request is written, never replay a possibly-applied method.
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep_until(
                    (tokio::time::Instant::now() + RETRY_INTERVAL).min(deadline),
                )
                .await;
            }
            // Missing service, access denial and an exhausted busy budget remain real errors.
            Err(error) => return Err(Error::new("SERVICE_UNAVAILABLE", error.to_string())),
        }
    }
}
pub async fn write_frame<W: AsyncWrite + Unpin>(io: &mut W, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err(Error::new("TOO_LARGE", "control frame exceeds limit"));
    }
    io.write_u32(bytes.len() as u32).await?;
    io.write_all(bytes).await?;
    io.flush().await?;
    Ok(())
}
pub async fn read_frame<R: AsyncRead + Unpin>(io: &mut R) -> Result<Vec<u8>> {
    let len = tokio::time::timeout(
        std::time::Duration::from_secs(crate::config::IDLE_SECONDS),
        io.read_u32(),
    )
    .await
    .map_err(|_| Error::new("TIMEOUT", "control frame timed out"))?? as usize;
    if len > MAX_CONTROL_BYTES {
        return Err(Error::new("TOO_LARGE", "control frame exceeds limit"));
    }
    let mut bytes = vec![0; len];
    tokio::time::timeout(
        std::time::Duration::from_secs(crate::config::IDLE_SECONDS),
        io.read_exact(&mut bytes),
    )
    .await
    .map_err(|_| Error::new("TIMEOUT", "control frame timed out"))??;
    Ok(bytes)
}
pub async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: S,
    engine: std::sync::Arc<crate::engine::Engine>,
) -> Result<()> {
    let request: Request = serde_json::from_slice(&read_frame(&mut socket).await?)?;
    let result = engine.dispatch(&request.method, request.params).await;
    let response = match result {
        Ok(v) => Response {
            request_id: request.request_id,
            result: Some(v),
            error: None,
        },
        Err(e) => Response {
            request_id: request.request_id,
            result: None,
            error: Some(e),
        },
    };
    write_frame(&mut socket, &serde_json::to_vec(&response)?).await
}
async fn read_response<R: AsyncRead + Unpin>(io: &mut R) -> Result<Vec<u8>> {
    let len = io.read_u32().await? as usize;
    if len > MAX_CONTROL_BYTES {
        return Err(Error::new("TOO_LARGE", "control frame exceeds limit"));
    }
    let mut bytes = vec![0; len];
    io.read_exact(&mut bytes).await?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn oversized_header_is_rejected_before_allocation() {
        let (mut a, mut b) = tokio::io::duplex(16);
        a.write_u32(MAX_CONTROL_BYTES as u32 + 1).await.unwrap();
        assert_eq!(read_frame(&mut b).await.unwrap_err().code, "TOO_LARGE");
    }

    // A real current-user pipe, not a mocked success, verifies the Windows accept/replacement
    // race. No request is sent to a possibly-applied method or retried after writing a frame.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_busy_pipe_waits_for_a_replacement_listener() -> Result<()> {
        let name =
            fta_platform::windows::user_pipe_name(&format!("busy-{}", uuid::Uuid::new_v4()))?;
        let occupied = fta_platform::windows::create_user_pipe(&name, true)?;
        let _first = tokio::net::windows::named_pipe::ClientOptions::new().open(&name)?;
        occupied.connect().await?;
        let connection = connect_windows_pipe(&name, std::time::Duration::from_secs(2));
        tokio::pin!(connection);
        tokio::select! {
            result = &mut connection => panic!("connection must wait while all instances are busy: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(30)) => {},
        }
        let mut replacement = fta_platform::windows::create_user_pipe(&name, false)?;
        let mut client = connection.await?;
        replacement.connect().await?;
        client.write_all(b"one").await?;
        let mut bytes = [0; 3];
        replacement.read_exact(&mut bytes).await?;
        assert_eq!(&bytes, b"one");
        Ok(())
    }

    // Bounded waiting must still report an unavailable service when the busy state never ends.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_busy_pipe_has_a_finite_connection_budget() -> Result<()> {
        let name = fta_platform::windows::user_pipe_name(&format!(
            "busy-timeout-{}",
            uuid::Uuid::new_v4()
        ))?;
        let occupied = fta_platform::windows::create_user_pipe(&name, true)?;
        let _first = tokio::net::windows::named_pipe::ClientOptions::new().open(&name)?;
        occupied.connect().await?;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            connect_windows_pipe(&name, std::time::Duration::from_millis(30)),
        )
        .await
        .expect("busy connection must remain bounded");
        assert_eq!(result.unwrap_err().code, "SERVICE_UNAVAILABLE");
        Ok(())
    }

    // Absence is not transient contention: a missing engine must fail promptly, not wait the
    // full busy budget or fabricate a connection that could enable offline transfer behavior.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_missing_pipe_is_not_retried() -> Result<()> {
        let name =
            fta_platform::windows::user_pipe_name(&format!("absent-{}", uuid::Uuid::new_v4()))?;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            connect_windows_pipe(&name, std::time::Duration::from_secs(5)),
        )
        .await
        .expect("missing pipe must fail without a busy wait");
        assert_eq!(result.unwrap_err().code, "SERVICE_UNAVAILABLE");
        Ok(())
    }
}
