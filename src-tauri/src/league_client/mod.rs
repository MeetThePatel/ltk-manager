pub mod models;

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// Parsed LCU lockfile data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockfileData {
    pub port: u16,
    pub password: String,
}

/// Read and parse the League Client lockfile.
/// Format: `LeagueClient:pid:port:password:https` (5-part) or `process:port:password:protocol` (4-part).
pub fn read_lockfile(league_path: &Path) -> Option<LockfileData> {
    let lockfile_path = resolve_lockfile_path(league_path);

    let content = match std::fs::read_to_string(&lockfile_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("Could not read lockfile at {:?}: {}", lockfile_path, e);
            return None;
        }
    };

    parse_lockfile_content(&content)
}

pub fn parse_lockfile_content(content: &str) -> Option<LockfileData> {
    let parts: Vec<&str> = content.trim().split(':').collect();

    match parts.len() {
        4 => {
            // Old format: process:port:password:protocol
            let port = parts[1].parse::<u16>().ok()?;
            let password = parts[2].to_string();
            tracing::debug!("Parsed lockfile (4-part): port={}", port);
            Some(LockfileData { port, password })
        }
        5 => {
            // New format: process:pid:port:password:protocol
            let port = parts[2].parse::<u16>().ok()?;
            let password = parts[3].to_string();
            tracing::debug!("Parsed lockfile (5-part): port={}", port);
            Some(LockfileData { port, password })
        }
        n if n > 5 => {
            // Try 5-part interpretation
            let port = parts[2].parse::<u16>().ok()?;
            let password = parts[3].to_string();
            tracing::warn!(
                "Lockfile has {} parts, using 5-part format guess: port={}",
                n,
                port
            );
            Some(LockfileData { port, password })
        }
        _ => {
            tracing::warn!("Invalid lockfile format: {} parts", parts.len());
            None
        }
    }
}

pub fn resolve_lockfile_path(league_path: &Path) -> PathBuf {
    let direct = league_path.join("lockfile");
    if direct.exists() {
        return direct;
    }

    #[cfg(target_os = "macos")]
    {
        let bundle_inner = league_path.join("Contents").join("LoL").join("lockfile");
        if bundle_inner.exists()
            || league_path.extension().and_then(|ext| ext.to_str()) == Some("app")
        {
            return bundle_inner;
        }
    }

    direct
}

pub struct LeagueClient {
    client: reqwest::blocking::Client,
    pub lockfile: LockfileData,
}

impl LeagueClient {
    pub fn new(league_path: &Path) -> Option<Self> {
        let lockfile = read_lockfile(league_path)?;
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_millis(1000))
            .build()
            .ok()?;
        Some(Self { client, lockfile })
    }

    pub fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        let url = format!("https://127.0.0.1:{}{}", self.lockfile.port, path);
        let response = self
            .client
            .get(&url)
            .basic_auth("riot", Some(&self.lockfile.password))
            .send()?;
        if response.status().is_success() {
            let val = response.json::<T>()?;
            Ok(val)
        } else {
            Err(AppError::Other(format!(
                "LCU request failed with status: {}",
                response.status()
            )))
        }
    }

    pub fn post_empty(&self, path: &str) -> AppResult<()> {
        let url = format!("https://127.0.0.1:{}{}", self.lockfile.port, path);
        let response = self
            .client
            .post(&url)
            .basic_auth("riot", Some(&self.lockfile.password))
            .header("Content-Type", "application/json")
            .send()?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Other(format!(
                "LCU POST request failed with status: {}",
                response.status()
            )))
        }
    }

    pub fn put_empty(&self, path: &str) -> AppResult<()> {
        let url = format!("https://127.0.0.1:{}{}", self.lockfile.port, path);
        let response = self
            .client
            .put(&url)
            .basic_auth("riot", Some(&self.lockfile.password))
            .header("Content-Type", "application/json")
            .send()?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Other(format!(
                "LCU PUT request failed with status: {}",
                response.status()
            )))
        }
    }
}

/// Attempt to reconnect to League via the LCU API (best-effort, non-fatal).
/// Retries several times with delays to give the client time to process the game exit.
pub fn try_lcu_reconnect(league_path: &Path) {
    let client = match LeagueClient::new(league_path) {
        Some(c) => c,
        None => {
            tracing::debug!("No lockfile or LCU client could be initialized, skipping reconnect");
            return;
        }
    };

    // Retry up to 5 times with increasing delays.
    // The League client needs time to process the game exit before it accepts reconnect.
    let retry_delays = [
        std::time::Duration::from_secs(3),
        std::time::Duration::from_secs(3),
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(5),
    ];

    let endpoints = [
        ("POST", "/lol-gameflow/v1/reconnect"),
        ("PUT", "/lol-gameflow/v1/reconnect"),
        ("POST", "/lol-login/v1/session/reconnect"),
    ];

    for (attempt, delay) in retry_delays.iter().enumerate() {
        tracing::debug!(
            "LCU reconnect: waiting {}s before attempt {} of {}",
            delay.as_secs(),
            attempt + 1,
            retry_delays.len()
        );
        std::thread::sleep(*delay);

        for (method, path) in &endpoints {
            tracing::debug!("Trying LCU reconnect: {} {}", method, path);
            let res = match *method {
                "POST" => client.post_empty(path),
                "PUT" => client.put_empty(path),
                _ => continue,
            };
            match res {
                Ok(_) => {
                    tracing::info!("LCU reconnect succeeded via {} {}", method, path);
                    return;
                }
                Err(e) => {
                    tracing::debug!("LCU {} {} failed: {}", method, path, e);
                }
            }
        }
    }

    tracing::debug!("LCU reconnect: all attempts exhausted (client may not need reconnect)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_lockfile_path_prefers_direct_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lockfile"), "LeagueClient:1:2:pw:https").unwrap();

        assert_eq!(
            resolve_lockfile_path(dir.path()),
            dir.path().join("lockfile")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolve_lockfile_path_uses_macos_app_inner_lol_dir() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("League of Legends.app");
        let inner = app.join("Contents").join("LoL");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("lockfile"), "LeagueClient:1:2:pw:https").unwrap();

        assert_eq!(resolve_lockfile_path(&app), inner.join("lockfile"));
    }

    #[test]
    fn test_parse_lockfile_content() {
        // 4-part parsing
        let data_4 = parse_lockfile_content("LeagueClient:1234:mypassword:https").unwrap();
        assert_eq!(data_4.port, 1234);
        assert_eq!(data_4.password, "mypassword");

        // 5-part parsing
        let data_5 = parse_lockfile_content("LeagueClient:9876:5678:anotherpw:https").unwrap();
        assert_eq!(data_5.port, 5678);
        assert_eq!(data_5.password, "anotherpw");

        // 6-part parsing (treated as 5-part guess)
        let data_6 =
            parse_lockfile_content("LeagueClient:9876:5678:anotherpw:https:extra").unwrap();
        assert_eq!(data_6.port, 5678);
        assert_eq!(data_6.password, "anotherpw");

        // invalid format
        assert!(parse_lockfile_content("invalid_format").is_none());
    }
}
