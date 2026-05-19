use crate::error::{AppError, AppResult};
use std::fs;
use std::io::{Read, Write};
#[cfg(ltk_macos_process_patcher_bundled)]
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const HELPER_NAME: &str = "ltk_macos_process_patcher";
const HELPER_PID_FILE: &str = "ltk_macos_process_patcher.pid";
const HELPER_LOG_FILE: &str = "ltk_macos_process_patcher.log";
const HELPER_SOCKET_FILE: &str = "ltk_macos_process_patcher.sock";
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const BROKER_START_TIMEOUT: Duration = Duration::from_secs(5);
const BROKER_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

#[cfg(ltk_macos_process_patcher_bundled)]
const BUNDLED_PROCESS_PATCHER: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/ltk_macos_process_patcher"));

pub fn run_process_patcher_loop(
    app_handle: &AppHandle,
    overlay_root: &Path,
    stop_flag: &AtomicBool,
) -> AppResult<()> {
    let helper = resolve_process_patcher(app_handle)?;
    tracing::info!(
        "macOS patcher: starting process patcher {} with overlay root {}",
        helper.display(),
        overlay_root.display()
    );

    let helper_process = ensure_process_patcher_broker(app_handle, &helper)?;
    helper_process.start_patching(overlay_root)?;
    loop {
        if !helper_process.is_running() {
            let log_tail = helper_process.read_log_tail();
            return Err(AppError::Other(format!(
                "macOS process patcher broker exited unexpectedly. Recent helper log:\n{}",
                log_tail
            )));
        }

        if stop_flag.load(Ordering::SeqCst) {
            tracing::info!("Stopping macOS process patcher");
            helper_process.stop_patching();
            return Ok(());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

struct ProcessPatcherChild {
    pid: u32,
    log_file: PathBuf,
    socket_file: PathBuf,
}

impl ProcessPatcherChild {
    fn is_running(&self) -> bool {
        let process_alive = Command::new("/bin/ps")
            .arg("-p")
            .arg(self.pid.to_string())
            .arg("-o")
            .arg("command=")
            .output()
            .map(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains(HELPER_NAME)
            })
            .unwrap_or(false);
        process_alive && send_broker_command(&self.socket_file, "ping").is_ok()
    }

    fn start_patching(&self, overlay_root: &Path) -> AppResult<()> {
        let response = send_broker_command(
            &self.socket_file,
            &format!("start {}", overlay_root.display()),
        )?;
        if response.starts_with("OK ") {
            Ok(())
        } else {
            Err(AppError::Other(format!(
                "macOS process patcher broker rejected start command: {}",
                response.trim()
            )))
        }
    }

    fn stop_patching(&self) {
        if let Err(e) = send_broker_command(&self.socket_file, "stop") {
            tracing::warn!(error = ?e, "Failed to stop macOS process patcher broker");
        }
    }

    fn read_log_tail(&self) -> String {
        let log = fs::read_to_string(&self.log_file).unwrap_or_default();
        let mut lines = log.lines().rev().take(30).collect::<Vec<_>>();
        lines.reverse();
        lines.join("\n")
    }
}

fn ensure_process_patcher_broker(
    app_handle: &AppHandle,
    helper: &Path,
) -> AppResult<ProcessPatcherChild> {
    let pid_file = process_patcher_pid_file(app_handle)?;
    let log_file = process_patcher_log_file(app_handle)?;
    let socket_file = process_patcher_socket_file(app_handle)?;

    if let Ok(pid) = read_pid_file(&pid_file) {
        let child = ProcessPatcherChild {
            pid,
            log_file: log_file.clone(),
            socket_file: socket_file.clone(),
        };
        if child.is_running() {
            return Ok(child);
        }
    }

    let _ = fs::remove_file(&pid_file);
    let _ = fs::remove_file(&socket_file);
    let _ = fs::remove_file(&log_file);

    spawn_elevated_process_patcher_broker(helper, &socket_file, &pid_file, &log_file)?;
    let pid = read_pid_file(&pid_file)?;
    let child = ProcessPatcherChild {
        pid,
        log_file,
        socket_file,
    };
    wait_for_broker_ready(&child)?;
    Ok(child)
}

fn spawn_elevated_process_patcher_broker(
    helper: &Path,
    socket_file: &Path,
    pid_file: &Path,
    log_file: &Path,
) -> AppResult<()> {
    let parent_pid = std::process::id();
    let shell_command = format!(
        "{} --parent-pid {} --broker-socket {} >> {} 2>&1 & printf %s $! > {}",
        shell_quote_path(helper),
        parent_pid,
        shell_quote_path(socket_file),
        shell_quote_path(log_file),
        shell_quote_path(pid_file),
    );
    let script = format!(
        "do shell script {} with administrator privileges",
        applescript_string(&shell_command)
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            AppError::Other(format!(
                "Failed to start elevated macOS process patcher {}: {}",
                helper.display(),
                e
            ))
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "Elevated macOS process patcher launch failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn read_pid_file(pid_file: &Path) -> AppResult<u32> {
    fs::read_to_string(pid_file)
        .map_err(|e| {
            AppError::Other(format!(
                "macOS process patcher did not write pid file {}: {}",
                pid_file.display(),
                e
            ))
        })?
        .trim()
        .parse::<u32>()
        .map_err(|e| AppError::Other(format!("Invalid macOS process patcher pid: {}", e)))
}

fn wait_for_broker_ready(child: &ProcessPatcherChild) -> AppResult<()> {
    let started = std::time::Instant::now();
    let mut last_error = None;
    while started.elapsed() < BROKER_START_TIMEOUT {
        match send_broker_command(&child.socket_file, "ping") {
            Ok(response) if response.starts_with("OK ") => return Ok(()),
            Ok(response) => {
                last_error = Some(format!("unexpected response: {}", response.trim()));
            }
            Err(e) => {
                last_error = Some(e.to_string());
            }
        }
        if !process_is_running(child.pid) {
            return Err(AppError::Other(format!(
                "macOS process patcher broker exited during startup. Recent helper log:\n{}",
                child.read_log_tail()
            )));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(AppError::Other(format!(
        "Timed out waiting for macOS process patcher broker socket {}. Last error: {}. Recent helper log:\n{}",
        child.socket_file.display(),
        last_error.unwrap_or_else(|| "<none>".to_string()),
        child.read_log_tail()
    )))
}

fn process_is_running(pid: u32) -> bool {
    Command::new("/bin/ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("command=")
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains(HELPER_NAME)
        })
        .unwrap_or(false)
}

fn send_broker_command(socket_file: &Path, command: &str) -> AppResult<String> {
    let mut stream = UnixStream::connect(socket_file).map_err(|e| {
        AppError::Other(format!(
            "Failed to connect to macOS process patcher broker {}: {}",
            socket_file.display(),
            e
        ))
    })?;
    stream.set_read_timeout(Some(BROKER_CONNECT_TIMEOUT))?;
    stream.set_write_timeout(Some(BROKER_CONNECT_TIMEOUT))?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.is_empty() {
        return Err(AppError::Other(
            "macOS process patcher broker returned an empty response".to_string(),
        ));
    }
    Ok(response)
}

fn process_patcher_pid_file(app_handle: &AppHandle) -> AppResult<PathBuf> {
    Ok(process_patcher_helper_dir(app_handle)?.join(HELPER_PID_FILE))
}

fn process_patcher_log_file(app_handle: &AppHandle) -> AppResult<PathBuf> {
    Ok(process_patcher_helper_dir(app_handle)?.join(HELPER_LOG_FILE))
}

fn process_patcher_socket_file(app_handle: &AppHandle) -> AppResult<PathBuf> {
    Ok(process_patcher_helper_dir(app_handle)?.join(HELPER_SOCKET_FILE))
}

fn process_patcher_helper_dir(app_handle: &AppHandle) -> AppResult<PathBuf> {
    let dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::Other(format!("Failed to resolve app cache directory: {}", e)))?
        .join("helpers");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.display().to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn resolve_process_patcher(app_handle: &AppHandle) -> AppResult<PathBuf> {
    if let Some(path) = extract_bundled_process_patcher(app_handle)? {
        return Ok(path);
    }

    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        candidates.push(resource_dir.join(HELPER_NAME));
        candidates.push(resource_dir.join("macos").join(HELPER_NAME));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(HELPER_NAME));
        }
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tools/macos-process-patcher")
            .join(HELPER_NAME),
    );

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    Err(AppError::Other(format!(
        "macOS process patcher helper not found. Build it with `make -C tools/macos-process-patcher`, then try again. Checked:\n - {}",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n - ")
    )))
}

#[cfg(ltk_macos_process_patcher_bundled)]
fn extract_bundled_process_patcher(app_handle: &AppHandle) -> AppResult<Option<PathBuf>> {
    if BUNDLED_PROCESS_PATCHER.is_empty() {
        return Ok(None);
    }

    let dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::Other(format!("Failed to resolve app cache directory: {}", e)))?
        .join("helpers");
    fs::create_dir_all(&dir)?;
    let path = dir.join(HELPER_NAME);

    let needs_write = fs::read(&path)
        .map(|current| current != BUNDLED_PROCESS_PATCHER)
        .unwrap_or(true);
    if needs_write {
        fs::write(&path, BUNDLED_PROCESS_PATCHER)?;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }

    Ok(Some(path))
}

#[cfg(not(ltk_macos_process_patcher_bundled))]
fn extract_bundled_process_patcher(_app_handle: &AppHandle) -> AppResult<Option<PathBuf>> {
    Ok(None)
}
