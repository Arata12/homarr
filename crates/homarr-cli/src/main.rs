use std::path::Path;
use std::time::Duration;

use clap::{Parser, Subcommand};
use homarr_auth::hash_password;
use homarr_common::{generate_secure_random_token, HomarrError};
use homarr_core::{AppConfig, DatabaseDriver};
use homarr_db::{
    repos::{groups, integrations, sessions, users},
    seed::seed_defaults,
    Database,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Parser)]
#[command(name = "homarr", version, about = "Homarr administration CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Checks whether the local Homarr HTTP server responds 200 on `/api/health/live`.
    Healthcheck {
        #[arg(long)]
        url: Option<String>,
    },
    Users {
        #[command(subcommand)]
        action: UsersCmd,
    },
    Integrations {
        #[command(subcommand)]
        action: IntegrationsCmd,
    },
    Db {
        #[command(subcommand)]
        action: DbCmd,
    },
}

#[derive(Subcommand)]
enum UsersCmd {
    List,
    Delete {
        id: String,
    },
    /// Sets a new random password (printed once) and signs the user out everywhere.
    ResetPassword {
        id: String,
        /// Deprecated: visible to other local users via the process list. Omit to generate one.
        password: Option<String>,
    },
    FixUsernames,
    /// Creates or resets a credentials admin with a random password (printed once).
    RecreateAdmin {
        username: String,
        /// Deprecated: visible to other local users via the process list. Omit to generate one.
        password: Option<String>,
    },
}

#[derive(Subcommand)]
enum IntegrationsCmd {
    List,
}

#[derive(Subcommand)]
enum DbCmd {
    Migrate,
    Seed,
}

fn ensure_sqlite_parent_dir(driver: DatabaseDriver, url: &str) {
    if driver != DatabaseDriver::Sqlite {
        return;
    }
    let clean = url
        .trim()
        .strip_prefix("sqlite://")
        .or_else(|| url.trim().strip_prefix("sqlite:"))
        .unwrap_or(url.trim());
    if clean.is_empty() || clean == ":memory:" || clean.starts_with("mode=memory") {
        return;
    }
    if let Some(parent) = Path::new(clean).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
}

fn parse_healthcheck_target(url: Option<&str>) -> Result<(String, u16, String), String> {
    let default_port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .unwrap_or(7575);
    let Some(raw) = url.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok((
            "127.0.0.1".to_string(),
            default_port,
            "/api/health/live".to_string(),
        ));
    };
    let rest = raw
        .strip_prefix("http://")
        .ok_or_else(|| format!("Healthcheck URL must start with http://: {raw}"))?;
    let (authority, path) = match rest.split_once('/') {
        Some((auth, p)) => (auth, format!("/{p}")),
        None => (rest, "/api/health/live".to_string()),
    };
    let (host, port) = if let Some(after_bracket) = authority.strip_prefix('[') {
        let (h, tail) = after_bracket
            .split_once(']')
            .ok_or_else(|| format!("Invalid IPv6 host in healthcheck URL: {raw}"))?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p
                .parse::<u16>()
                .map_err(|_| format!("Invalid port in healthcheck URL: {raw}"))?,
            None => 80,
        };
        (h.to_string(), port)
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        let port = p
            .parse::<u16>()
            .map_err(|_| format!("Invalid port in healthcheck URL: {raw}"))?;
        (h.to_string(), port)
    } else {
        (authority.to_string(), 80)
    };
    if host.is_empty() {
        return Err(format!("Missing host in healthcheck URL: {raw}"));
    }
    Ok((host, port, path))
}

/// Returns the password to set and whether it was generated (and so must be printed).
fn resolve_password(supplied: Option<String>) -> Result<(String, bool), HomarrError> {
    let Some(password) = supplied else {
        // 24 random bytes hex-encoded = 48 characters, as upstream.
        return Ok((generate_secure_random_token(24), true));
    };
    eprintln!(
        "warning: passwords given on the command line are visible to other users (process list, shell history); omit it to generate one"
    );
    if password.chars().count() < 8 {
        return Err(HomarrError::BadRequest(
            "Password must be >= 8 chars".into(),
        ));
    }
    Ok((password, false))
}

async fn run_healthcheck(url: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let (host, port, path) = parse_healthcheck_target(url).map_err(HomarrError::BadRequest)?;
    let probe = async {
        let mut stream = TcpStream::connect((host.as_str(), port)).await?;
        let host_header = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let req =
            format!("GET {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).await?;
        let mut buf = [0u8; 512];
        let n = stream.read(&mut buf).await?;
        let head = String::from_utf8_lossy(&buf[..n]);
        let status_line = head.lines().next().unwrap_or("");
        if status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200") {
            Ok::<(), std::io::Error>(())
        } else {
            Err(std::io::Error::other(format!(
                "Unexpected healthcheck status: {status_line}"
            )))
        }
    };
    tokio::time::timeout(Duration::from_secs(3), probe)
        .await
        .map_err(|_| HomarrError::Upstream("Healthcheck timed out".into()))??;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    if let Commands::Healthcheck { url } = &cli.command {
        return run_healthcheck(url.as_deref()).await;
    }

    let cfg = AppConfig::from_env()?;
    ensure_sqlite_parent_dir(cfg.db_driver, &cfg.db_url);
    let db = Database::connect(cfg.db_driver, &cfg.db_url).await?;
    db.migrate().await?;

    match cli.command {
        Commands::Healthcheck { .. } => unreachable!(),
        Commands::Users { action } => match action {
            UsersCmd::List => {
                let all = users::list_all(&db).await?;
                println!("{}", serde_json::to_string_pretty(&all)?);
            }
            UsersCmd::Delete { id } => {
                let ok = users::delete(&db, &id).await?;
                println!("deleted={ok}");
            }
            UsersCmd::ResetPassword { id, password } => {
                let user = users::find_by_id(&db, &id)
                    .await?
                    .ok_or_else(|| HomarrError::NotFound(format!("User {id} not found")))?;
                if user.provider != "credentials" {
                    let msg = format!("User {id} does not use the credentials provider");
                    return Err(msg.into());
                }
                let (password, generated) = resolve_password(password)?;
                let hash = hash_password(&password).await?;
                users::update_password(&db, &id, &hash).await?;
                let removed = sessions::delete_by_user_id(&db, &id).await?;
                println!("Password reset for user {id}; deleted {removed} session(s)");
                if generated {
                    println!("New password: {password}");
                }
            }
            UsersCmd::FixUsernames => {
                let count = users::fix_usernames(&db).await?;
                println!("Normalized {count} usernames");
            }
            UsersCmd::RecreateAdmin { username, password } => {
                let existing = users::find_by_name_ci(&db, &username).await?;
                if matches!(&existing, Some(u) if u.provider != "credentials") {
                    let msg = format!("User {username} does not use the credentials provider");
                    return Err(msg.into());
                }
                let (password, generated) = resolve_password(password)?;
                seed_defaults(&db).await?;
                let hash = hash_password(&password).await?;
                let user = match existing {
                    Some(existing) => {
                        users::update_password(&db, &existing.id, &hash).await?;
                        sessions::delete_by_user_id(&db, &existing.id).await?;
                        existing
                    }
                    None => {
                        users::create(
                            &db,
                            users::CreateUserInput {
                                id: None,
                                name: username,
                                email: None,
                                password_hash: Some(hash),
                                provider: "credentials".to_string(),
                                image: None,
                            },
                        )
                        .await?
                    }
                };
                let admin_group = match groups::get_by_name(&db, "administrators").await? {
                    Some(g) => g,
                    None => {
                        let g = groups::create(&db, "administrators", Some(&user.id), 0).await?;
                        groups::set_permissions(&db, &g.id, &["admin".to_string()]).await?;
                        g
                    }
                };
                groups::add_member(&db, &admin_group.id, &user.id).await?;
                if let Some(everyone) = groups::get_by_name(&db, "everyone").await? {
                    let _ = groups::add_member(&db, &everyone.id, &user.id).await;
                }
                println!(
                    "Admin user '{}' ({}) ready",
                    user.name.unwrap_or_default(),
                    user.id
                );
                if generated {
                    println!("Password: {password}");
                }
            }
        },
        Commands::Integrations { action } => match action {
            IntegrationsCmd::List => {
                let all = integrations::list_all(&db).await?;
                println!("{}", serde_json::to_string_pretty(&all)?);
            }
        },
        Commands::Db { action } => match action {
            DbCmd::Migrate => {
                db.migrate().await?;
                println!("Database migrations applied");
            }
            DbCmd::Seed => {
                seed_defaults(&db).await?;
                println!("Default database records seeded");
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_healthcheck_target;

    #[test]
    fn parses_healthcheck_urls() {
        assert_eq!(
            parse_healthcheck_target(Some("http://127.0.0.1:8080/api/health/live")).unwrap(),
            (
                "127.0.0.1".to_string(),
                8080,
                "/api/health/live".to_string()
            )
        );
        assert_eq!(
            parse_healthcheck_target(Some("http://[::1]:7575")).unwrap(),
            ("::1".to_string(), 7575, "/api/health/live".to_string())
        );
        assert!(parse_healthcheck_target(Some("https://example.com")).is_err());
    }
}
