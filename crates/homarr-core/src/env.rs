use std::path::{Path, PathBuf};

use homarr_common::{generate_secure_random_token, HomarrError, HomarrResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseDriver {
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub base_url: Option<String>,
    pub db_driver: DatabaseDriver,
    pub db_url: String,
    pub secret_encryption_key: String,
    pub auth_secret: String,
    pub auth_cookie_prefix: String,
    /// Explicit `AUTH_COOKIE_SECURE` override for the session cookie `Secure` flag.
    pub auth_cookie_secure: Option<bool>,
    pub auth_providers: Vec<String>,
    pub auth_session_expiry_secs: i64,
    pub auth_trusted_proxy_header: Option<String>,
    /// IPs/CIDRs allowed to set `auth_trusted_proxy_header`; empty means the header is ignored.
    pub auth_trusted_proxy_ips: Vec<String>,
    pub redis_is_external: bool,
    pub redis_host: String,
    pub redis_port: u16,
    pub redis_username: Option<String>,
    pub redis_password: Option<String>,
    pub redis_db: i64,
    pub redis_disabled: bool,
    pub log_level: String,
    pub local_certificate_path: PathBuf,
    pub docker_socket_paths: Vec<String>,
    pub docker_hostnames: Vec<String>,
    pub docker_ports: Vec<u16>,
    pub enable_kubernetes: bool,
    pub demo_mode: bool,
    pub demo_read_only: bool,
    pub unsafe_enable_mock_integration: bool,
    pub no_external_connection: bool,
    pub workshop_api_url: String,
    pub workshop_web_url: String,
    pub assets_dir: PathBuf,
    pub static_data_dir: PathBuf,
    pub homarr_request_identity_secret: Option<String>,
}

fn parse_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(val) => matches!(
            val.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn parse_csv(key: &str) -> Vec<String> {
    std::env::var(key)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_bool_opt(key: &str) -> Option<bool> {
    let val = non_empty_env(key)?;
    match val.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn is_valid_hex_64(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_non_empty(value: &str) -> bool {
    !value.is_empty()
}

/// Loads a persisted secret from `<dir>/secrets/<file_name>`, generating and persisting a new one
/// atomically (mode 0600 on unix) only when the file does not exist. An unreadable or invalid
/// existing file, or a failed write, is an error: silently replacing the secret would make data
/// protected by the old one unreadable. The secret value is never logged.
fn load_or_create_secret(
    dir: &Path,
    file_name: &str,
    label: &str,
    validate: fn(&str) -> bool,
) -> HomarrResult<String> {
    let path = dir.join("secrets").join(file_name);
    match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let trimmed = existing.trim();
            if validate(trimmed) {
                return Ok(trimmed.to_string());
            }
            Err(HomarrError::Internal(format!(
                "{label} is not set and the persisted value at {} is empty or invalid. Refusing to replace it because data protected by the previous value would become unreadable. Set {label} (or {label}_FILE) to the correct value, or remove the file to generate a new one.",
                path.display()
            )))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let generated = generate_secure_random_token(32);
            write_secret_file(&path, &generated).map_err(|e| {
                HomarrError::Internal(format!(
                    "{label} is not set and a generated value could not be persisted at {} ({e}). Make the directory writable or set {label} (or {label}_FILE) explicitly.",
                    path.display()
                ))
            })?;
            eprintln!(
                "Warning: {label} is not set; generated a value and persisted it at {}. Back up this file or set {label} explicitly.",
                path.display()
            );
            Ok(generated)
        }
        Err(e) => Err(HomarrError::Internal(format!(
            "{label} is not set and the persisted value at {} could not be read ({e}). Fix the file permissions or set {label} (or {label}_FILE) explicitly.",
            path.display()
        ))),
    }
}

/// Reads `key` from the environment, falling back to the trimmed contents of the file named by
/// `<key>_FILE` (Docker secrets). The plain variable takes precedence.
fn secret_env(key: &str) -> HomarrResult<Option<String>> {
    if let Some(value) = non_empty_env(key) {
        return Ok(Some(value));
    }
    let file_key = format!("{key}_FILE");
    let Some(file_path) = non_empty_env(&file_key) else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(&file_path).map_err(|e| {
        HomarrError::Internal(format!(
            "{file_key} is set to {file_path} but the file could not be read ({e})"
        ))
    })?;
    let value = content.trim();
    if value.is_empty() {
        return Err(HomarrError::Internal(format!(
            "{file_key} is set to {file_path} but the file is empty"
        )));
    }
    Ok(Some(value.to_string()))
}

fn write_secret_file(path: &Path, value: &str) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("secret");
    let nonce = generate_secure_random_token(8);
    let tmp_path = parent.join(format!(".tmp-{file_name}-{}-{nonce}", std::process::id()));

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }

    let res = (|| -> std::io::Result<()> {
        let mut file = opts.open(&tmp_path)?;
        file.write_all(value.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    res
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

impl AppConfig {
    pub fn from_env() -> HomarrResult<Self> {
        let dialect_raw = non_empty_env("DB_DIALECT")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let driver_raw = non_empty_env("DB_DRIVER")
            .unwrap_or_else(|| "sqlite".to_string())
            .to_ascii_lowercase();
        let url_raw = non_empty_env("DB_URL");

        if matches!(dialect_raw.as_str(), "mysql" | "mariadb")
            || matches!(driver_raw.as_str(), "mysql" | "mysql2" | "mariadb")
            || url_raw
                .as_deref()
                .map(|u| {
                    let lower = u.to_ascii_lowercase();
                    lower.starts_with("mysql:") || lower.starts_with("mariadb:")
                })
                .unwrap_or(false)
        {
            return Err(HomarrError::BadRequest(
                "MySQL and MariaDB are no longer supported in Homarr v2. Migrate your database to SQLite or PostgreSQL before starting Homarr.".to_string(),
            ));
        }

        let db_driver = match driver_raw.as_str() {
            "sqlite" | "better-sqlite3" => DatabaseDriver::Sqlite,
            "postgres" | "postgresql" | "node-postgres" => DatabaseDriver::Postgres,
            other => {
                return Err(HomarrError::BadRequest(format!(
                    "Invalid DB_DRIVER '{other}', supported values are sqlite, better-sqlite3, postgres, postgresql, node-postgres"
                )));
            }
        };

        let db_url = match (db_driver, url_raw) {
            (DatabaseDriver::Sqlite, Some(u)) => u,
            (DatabaseDriver::Sqlite, None) => "/appdata/db/db.sqlite".to_string(),
            (DatabaseDriver::Postgres, Some(u)) => u,
            (DatabaseDriver::Postgres, None) => {
                let host = non_empty_env("DB_HOST").ok_or_else(|| {
                    HomarrError::BadRequest(
                        "DB_URL or DB_HOST is required for PostgreSQL".to_string(),
                    )
                })?;
                let port = non_empty_env("DB_PORT").unwrap_or_else(|| "5432".to_string());
                let user = non_empty_env("DB_USER").unwrap_or_else(|| "homarr".to_string());
                let pass = non_empty_env("DB_PASSWORD").unwrap_or_default();
                let name = non_empty_env("DB_NAME").unwrap_or_else(|| "homarrdb".to_string());
                format!("postgres://{user}:{pass}@{host}:{port}/{name}")
            }
        };

        let static_data_dir = non_empty_env("STATIC_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("static-data"));

        let secret_encryption_key = match secret_env("SECRET_ENCRYPTION_KEY")? {
            Some(key) => {
                if !is_valid_hex_64(&key) {
                    let candidate = generate_secure_random_token(32);
                    return Err(HomarrError::BadRequest(format!(
                        "SECRET_ENCRYPTION_KEY must be a 64-character hex string; candidate: \"{candidate}\""
                    )));
                }
                key
            }
            None => load_or_create_secret(
                &static_data_dir,
                "secret_encryption_key",
                "SECRET_ENCRYPTION_KEY",
                is_valid_hex_64,
            )?,
        };

        let auth_secret = match secret_env("AUTH_SECRET")? {
            Some(secret) => secret,
            None => {
                load_or_create_secret(&static_data_dir, "auth_secret", "AUTH_SECRET", is_non_empty)?
            }
        };

        let host = non_empty_env("HOST").unwrap_or_else(|| "0.0.0.0".to_string());
        let port = non_empty_env("PORT")
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(7575);

        let redis_host = non_empty_env("REDIS_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
        let redis_port = non_empty_env("REDIS_PORT")
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(6379);
        let redis_db = non_empty_env("REDIS_DATABASE_INDEX")
            .and_then(|p| p.parse::<i64>().ok())
            .unwrap_or(0);

        let auth_providers = {
            let parsed = parse_csv("AUTH_PROVIDERS");
            if parsed.is_empty() {
                vec!["credentials".to_string()]
            } else {
                parsed
            }
        };

        let auth_session_expiry_secs = non_empty_env("AUTH_SESSION_EXPIRY_TIME")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(30 * 24 * 60 * 60);

        let docker_socket_paths = {
            let paths = parse_csv("DOCKER_SOCKET_PATHS");
            if paths.is_empty() && parse_csv("DOCKER_HOSTNAMES").is_empty() {
                vec!["/var/run/docker.sock".to_string()]
            } else {
                paths
            }
        };
        let docker_hostnames = parse_csv("DOCKER_HOSTNAMES");
        let docker_ports = parse_csv("DOCKER_PORTS")
            .into_iter()
            .filter_map(|p| p.parse::<u16>().ok())
            .collect();

        let website_url = non_empty_env("HOMARR_WEBSITE_URL")
            .unwrap_or_else(|| "https://homarr.dev".to_string())
            .trim_end_matches('/')
            .to_string();
        let workshop_api_url = non_empty_env("WORKSHOP_API_URL")
            .unwrap_or_else(|| website_url.clone())
            .trim_end_matches('/')
            .to_string();
        let workshop_web_url = non_empty_env("WORKSHOP_WEB_URL")
            .unwrap_or_else(|| format!("{website_url}/workshop"))
            .trim_end_matches('/')
            .to_string();

        let local_certificate_path = non_empty_env("LOCAL_CERTIFICATE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/appdata/trusted-certificates"));

        let assets_dir = non_empty_env("ASSETS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("assets"));

        Ok(Self {
            host,
            port,
            base_url: non_empty_env("BASE_URL").map(|u| u.trim_end_matches('/').to_string()),
            db_driver,
            db_url,
            secret_encryption_key,
            auth_secret,
            auth_cookie_prefix: non_empty_env("AUTH_COOKIE_PREFIX")
                .unwrap_or_else(|| "homarr".to_string()),
            auth_cookie_secure: parse_bool_opt("AUTH_COOKIE_SECURE"),
            auth_providers,
            auth_session_expiry_secs,
            auth_trusted_proxy_header: non_empty_env("AUTH_TRUSTED_PROXY_HEADER"),
            auth_trusted_proxy_ips: non_empty_env("AUTH_TRUSTED_PROXY_IPS")
                .map(|v| {
                    v.split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            redis_is_external: parse_bool("REDIS_IS_EXTERNAL", false),
            redis_host,
            redis_port,
            redis_username: non_empty_env("REDIS_USERNAME"),
            redis_password: non_empty_env("REDIS_PASSWORD"),
            redis_db,
            redis_disabled: parse_bool("REDIS_DISABLED", false),
            log_level: non_empty_env("LOG_LEVEL").unwrap_or_else(|| "info".to_string()),
            local_certificate_path,
            docker_socket_paths,
            docker_hostnames,
            docker_ports,
            enable_kubernetes: parse_bool("ENABLE_KUBERNETES", false),
            demo_mode: parse_bool("DEMO_MODE", false),
            demo_read_only: parse_bool("DEMO_READ_ONLY", true),
            unsafe_enable_mock_integration: parse_bool("UNSAFE_ENABLE_MOCK_INTEGRATION", false),
            no_external_connection: parse_bool("NO_EXTERNAL_CONNECTION", false),
            workshop_api_url,
            workshop_web_url,
            assets_dir,
            static_data_dir,
            homarr_request_identity_secret: non_empty_env("HOMARR_REQUEST_IDENTITY_SECRET"),
        })
    }

    /// Returns a safe test/default configuration (in-memory SQLite, zero encryption key, Redis disabled).
    pub fn for_test() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 0,
            base_url: None,
            db_driver: DatabaseDriver::Sqlite,
            db_url: "sqlite::memory:".to_string(),
            secret_encryption_key: "0".repeat(64),
            auth_secret: "1".repeat(64),
            auth_cookie_prefix: "homarr".to_string(),
            auth_cookie_secure: None,
            auth_providers: vec!["credentials".to_string()],
            auth_session_expiry_secs: 86_400,
            auth_trusted_proxy_header: None,
            auth_trusted_proxy_ips: Vec::new(),
            redis_is_external: false,
            redis_host: "127.0.0.1".to_string(),
            redis_port: 6379,
            redis_username: None,
            redis_password: None,
            redis_db: 0,
            redis_disabled: true,
            log_level: "info".to_string(),
            local_certificate_path: PathBuf::from("./test-certs"),
            docker_socket_paths: Vec::new(),
            docker_hostnames: Vec::new(),
            docker_ports: Vec::new(),
            enable_kubernetes: false,
            demo_mode: false,
            demo_read_only: true,
            unsafe_enable_mock_integration: true,
            no_external_connection: true,
            workshop_api_url: "https://homarr.dev".to_string(),
            workshop_web_url: "https://homarr.dev/workshop".to_string(),
            assets_dir: PathBuf::from("assets"),
            static_data_dir: PathBuf::from("static-data"),
            homarr_request_identity_secret: None,
        }
    }

    pub fn redis_url(&self) -> String {
        let auth = match (&self.redis_username, &self.redis_password) {
            (Some(u), Some(p)) => format!("{u}:{p}@"),
            (None, Some(p)) => format!(":{p}@"),
            (Some(u), None) => format!("{u}@"),
            (None, None) => String::new(),
        };
        format!(
            "redis://{auth}{}:{}/{}",
            self.redis_host, self.redis_port, self.redis_db
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_for_test_builds_valid_redis_url() {
        let cfg = AppConfig::for_test();
        assert_eq!(cfg.redis_url(), "redis://127.0.0.1:6379/0");
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "homarr-{name}-{}-{}",
            std::process::id(),
            generate_secure_random_token(4)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn generated_secret_is_persisted_and_reloaded() {
        let dir = unique_temp_dir("secret-test");
        let first = load_or_create_secret(&dir, "k", "TEST", is_valid_hex_64).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(
            std::fs::read_to_string(dir.join("secrets/k")).unwrap(),
            first
        );
        assert_eq!(
            first,
            load_or_create_secret(&dir, "k", "TEST", is_valid_hex_64).unwrap()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("secrets/k"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_persisted_secret_is_an_error_and_left_untouched() {
        let dir = unique_temp_dir("secret-invalid");
        std::fs::create_dir_all(dir.join("secrets")).unwrap();
        let path = dir.join("secrets/secret_encryption_key");

        for content in ["", "not-a-valid-hex-key"] {
            std::fs::write(&path, content).unwrap();
            let result =
                load_or_create_secret(&dir, "secret_encryption_key", "SECRET", is_valid_hex_64);
            assert!(result.is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_valid_persisted_secret_is_reused() {
        let dir = unique_temp_dir("secret-existing");
        std::fs::create_dir_all(dir.join("secrets")).unwrap();
        let key = "ab".repeat(32);
        std::fs::write(
            dir.join("secrets/secret_encryption_key"),
            format!("{key}\n"),
        )
        .unwrap();

        let loaded =
            load_or_create_secret(&dir, "secret_encryption_key", "SECRET", is_valid_hex_64)
                .unwrap();
        assert_eq!(loaded, key);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
