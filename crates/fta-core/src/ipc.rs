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
    let mut socket = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(paths.pipe_name()?)
        .map_err(|e| Error::new("SERVICE_UNAVAILABLE", e.to_string()))?;
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
}
