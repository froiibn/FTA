//! A one-shot helper transport binds both pipe endpoints to the intended processes.
use super::native::{Apartment, error};
use crate::windows::{create_user_pipe, user_pipe_name};
use std::{io, mem, os::windows::io::AsRawHandle, path::Path, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
        Threading::{
            CreateMutexW, GetExitCodeProcess, GetProcessId, ReleaseMutex, WaitForSingleObject,
        },
    },
    UI::{
        Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};

const INSTANCE_PREFIX: &str = "network-";
const MAX_FRAME: usize = 64 * 1024;
const HELPER_TIMEOUT: Duration = Duration::from_secs(120);
const PROCESS_POLL: Duration = Duration::from_millis(100);
const STILL_ACTIVE: u32 = 259;

// UI guards are process-local; a named same-user mutation lock also serializes independent
// elevated helpers so two app instances cannot create duplicate or interleaved owned rules.
pub(super) struct MutationLock(HANDLE);
impl MutationLock {
    pub(super) fn acquire() -> io::Result<Self> {
        Self::named(&format!(
            r"Local\FTA.NetworkSetup.{}",
            crate::windows::current_user_sid()?
        ))
    }
    fn named(name: &str) -> io::Result<Self> {
        let name = wide(name)?;
        // SAFETY: name is terminated; the kernel applies the caller token's default DACL.
        // This is a per-session, SID-named lock, not an IPC channel carrying privileged input.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: handle owns a valid mutex; a zero timeout never blocks another operation.
        let wait = unsafe { WaitForSingleObject(handle, 0) };
        if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
            return Ok(Self(handle));
        }
        let error = if wait == WAIT_TIMEOUT {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "another network setup helper is active",
            )
        } else {
            io::Error::last_os_error()
        };
        // SAFETY: no ownership was acquired, so only close this handle without releasing.
        unsafe { CloseHandle(handle) };
        Err(error)
    }
}
impl Drop for MutationLock {
    fn drop(&mut self) {
        // SAFETY: this guard owns the mutex on its current thread. Its raw handle is not Send,
        // and the helper's mutation handler remains synchronous until the guard is dropped.
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

struct Process(HANDLE);
impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the guard owns the handle returned by a successful ShellExecuteExW call.
        unsafe { CloseHandle(self.0) };
    }
}
fn validate_instance(instance: &str) -> io::Result<()> {
    let uuid = instance
        .strip_prefix(INSTANCE_PREFIX)
        .ok_or_else(|| error("invalid helper pipe instance"))?;
    if uuid::Uuid::parse_str(uuid).is_err() || uuid.len() != 36 {
        return Err(error("invalid helper pipe instance"));
    }
    Ok(())
}
fn wide(value: &str) -> io::Result<Vec<u16>> {
    if value.contains('\0') {
        return Err(error("helper launch value contains NUL"));
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}
fn launch(helper: &Path, instance: &str) -> io::Result<Process> {
    validate_instance(instance)?;
    let _apartment = Apartment::enter()?;
    let verb = wide("runas")?;
    let executable = wide(
        helper
            .to_str()
            .ok_or_else(|| error("helper path is not Unicode"))?,
    )?;
    // Only a generated UUID and this process's numeric PID enter the command line; neither
    // paths/CIDRs nor any user-provided command can cross the UAC boundary through arguments.
    let arguments = wide(&format!("--apply {instance} {}", std::process::id()))?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: executable.as_ptr(),
        lpParameters: arguments.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    // SAFETY: all strings and outputs remain live until the synchronous OS runas request
    // returns. UAC is deliberately left to Windows and the user; there is no consent bypass.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.hProcess.is_null() {
        return Err(error("Windows returned no helper process"));
    }
    Ok(Process(info.hProcess))
}
async fn process_exited(process: &Process) -> io::Error {
    loop {
        let mut code = 0;
        // SAFETY: Process keeps the real helper handle alive for the entire query loop.
        if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
            return io::Error::last_os_error();
        }
        if code != STILL_ACTIVE {
            return error(format!("network helper exited before connecting ({code})"));
        }
        tokio::time::sleep(PROCESS_POLL).await;
    }
}

pub(super) fn exchange(helper: &Path, request: &[u8]) -> io::Result<Vec<u8>> {
    if request.is_empty() || request.len() > MAX_FRAME {
        return Err(error("invalid network setup request size"));
    }
    // This synchronous API belongs on the desktop's blocking pool. The private runtime keeps
    // pipe timeouts independent from the UI thread and does not run a privileged service.
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let instance = format!("{INSTANCE_PREFIX}{}", uuid::Uuid::new_v4());
        let pipe_name = user_pipe_name(&instance)?;
        let mut server = create_user_pipe(&pipe_name, true)?;
        let process = launch(helper, &instance)?;
        tokio::select! {
            result = server.connect() => result?,
            error = process_exited(&process) => return Err(error),
            _ = tokio::time::sleep(HELPER_TIMEOUT) => return Err(error("network helper connection timed out")),
        }
        let mut client_pid = 0;
        // SAFETY: server owns a connected pipe; process owns the exact process launched above.
        if unsafe { GetNamedPipeClientProcessId(server.as_raw_handle().cast(), &mut client_pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: GetProcessId borrows the still-live helper handle.
        if client_pid != unsafe { GetProcessId(process.0) } { return Err(error("unexpected network helper pipe client")); }
        let mut started = false;
        let result = tokio::time::timeout(HELPER_TIMEOUT, async {
            write_frame(&mut server, request).await?;
            if server.read_u8().await? != 1 { return Err(error("network helper rejected the request")); }
            // The helper must observe this acknowledgement before mutating OS state. If the
            // desktop has exited or cancelled the connection, validation cannot become apply.
            started = true;
            server.write_u8(1).await?;
            server.flush().await?;
            read_frame(&mut server).await
        }).await.unwrap_or_else(|_| Err(error("network helper response timed out; inspect actual state before retrying")));
        // Once the apply acknowledgement could have reached the helper, a lost reply is an
        // uncertain/partial outcome, never proof that no OS changes occurred.
        result.map_err(|e| if started { io::Error::new(io::ErrorKind::ConnectionAborted, e) } else { e })
    })
}

pub(super) fn serve(
    instance: &str,
    parent_pid: u32,
    apply: impl FnOnce(&[u8]) -> io::Result<Vec<u8>>,
) -> io::Result<()> {
    validate_instance(instance)?;
    if parent_pid == 0 {
        return Err(error("invalid network helper parent process"));
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            // Building the name from this helper's own SID intentionally rejects over-the-shoulder
            // elevation under a different account rather than guessing another user's authority.
            let pipe_name = user_pipe_name(instance)?;
            let mut client =
                tokio::net::windows::named_pipe::ClientOptions::new().open(&pipe_name)?;
            let mut server_pid = 0;
            // SAFETY: client owns a connected pipe and server_pid is a writable output.
            if unsafe {
                GetNamedPipeServerProcessId(client.as_raw_handle().cast(), &mut server_pid)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if server_pid != parent_pid {
                return Err(error("network helper pipe belongs to another process"));
            }
            let request = tokio::time::timeout(HELPER_TIMEOUT, read_frame(&mut client))
                .await
                .map_err(|_| error("network helper request timed out"))??;
            client.write_u8(1).await?;
            client.flush().await?;
            let acknowledgement = tokio::time::timeout(HELPER_TIMEOUT, client.read_u8())
                .await
                .map_err(|_| error("network helper confirmation timed out"))??;
            if acknowledgement != 1 {
                return Err(error("network helper confirmation was cancelled"));
            }
            // The fixed binary's sole handler independently validates the plan, current network,
            // installed engine and administrator token; pipe data alone grants no arbitrary action.
            let reply = apply(&request)?;
            write_frame(&mut client, &reply).await?;
            client.flush().await
        })
}

async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<Vec<u8>> {
    let size = reader.read_u32().await? as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(error("network helper frame exceeds the allowed size"));
    }
    let mut body = vec![0; size];
    reader.read_exact(&mut body).await?;
    Ok(body)
}
async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), body: &[u8]) -> io::Result<()> {
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(error("network helper frame exceeds the allowed size"));
    }
    writer.write_u32(body.len() as u32).await?;
    writer.write_all(body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reject malformed lengths from the header before allocating or waiting for any body.
    #[tokio::test]
    async fn oversized_frame_is_rejected_without_a_body() -> io::Result<()> {
        let (mut sender, mut receiver) = tokio::io::duplex(16);
        sender.write_u32(MAX_FRAME as u32 + 1).await?;
        assert!(read_frame(&mut receiver).await.is_err());
        Ok(())
    }
    // Helper arguments are identifiers, never generic command/path/cidr input.
    #[test]
    fn helper_instance_is_a_single_generated_identifier() {
        assert!(validate_instance(&format!("network-{}", uuid::Uuid::new_v4())).is_ok());
        for invalid in [
            "network-../file",
            "network-192.168.0.0/24",
            "network-id --command calc",
            "network-",
        ] {
            assert!(validate_instance(invalid).is_err());
        }
    }

    // The real kernel lock serializes separate workers without UAC or persistent OS changes.
    #[test]
    fn mutation_lock_excludes_another_worker() -> io::Result<()> {
        let name = format!(r"Local\FTA.NetworkSetup.test.{}", uuid::Uuid::new_v4());
        let guard = MutationLock::named(&name)?;
        let other = name.clone();
        let blocked =
            std::thread::spawn(move || MutationLock::named(&other).err().map(|e| e.kind()))
                .join()
                .map_err(|_| error("lock test worker failed"))?;
        assert_eq!(blocked, Some(io::ErrorKind::WouldBlock));
        drop(guard);
        let _again = MutationLock::named(&name)?;
        Ok(())
    }

    // The transport preserves bytes exactly; a JSON parser belongs to the fixed typed handler.
    #[tokio::test]
    async fn bounded_frame_round_trip() -> io::Result<()> {
        let (mut sender, mut receiver) = tokio::io::duplex(128);
        let body = br#"{"operation":"validated-home-plan"}"#;
        write_frame(&mut sender, body).await?;
        assert_eq!(read_frame(&mut receiver).await?, body);
        Ok(())
    }
}
