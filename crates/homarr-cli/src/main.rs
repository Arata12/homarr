use std::path::Path;

use clap::{Parser, Subcommand};
use homarr_auth::hash_password;
use homarr_common::HomarrError;
use homarr_core::{AppConfig, DatabaseDriver};
use homarr_db::{
    repos::{groups, integrations, users},
    seed::seed_defaults,
    Database,
};

#[derive(Parser)]
#[command(name = "homarr", version, about = "Homarr administration CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
    Delete { id: String },
    ResetPassword { id: String, password: String },
    FixUsernames,
    RecreateAdmin { username: String, password: String },
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let cfg = AppConfig::from_env()?;
    ensure_sqlite_parent_dir(cfg.db_driver, &cfg.db_url);
    let db = Database::connect(cfg.db_driver, &cfg.db_url).await?;
    db.migrate().await?;

    match cli.command {
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
                if users::find_by_id(&db, &id).await?.is_none() {
                    return Err(HomarrError::NotFound(format!("User {id} not found")).into());
                }
                let hash = hash_password(&password).await?;
                users::update_password(&db, &id, &hash).await?;
                println!("Password reset for user {id}");
            }
            UsersCmd::FixUsernames => {
                let count = users::fix_usernames(&db).await?;
                println!("Normalized {count} usernames");
            }
            UsersCmd::RecreateAdmin { username, password } => {
                seed_defaults(&db).await?;
                let hash = hash_password(&password).await?;
                let user = match users::find_by_name_ci(&db, &username).await? {
                    Some(existing) => {
                        users::update_password(&db, &existing.id, &hash).await?;
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
