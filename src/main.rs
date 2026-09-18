mod app;
mod bluesky;
mod config;
mod db;
mod domain;
mod http;
mod import;
mod streetview;
#[cfg(test)]
mod tests;
mod twitter;
use anyhow::{Result, ensure};
use clap::{Args, Parser, Subcommand};
use config::{Config, Platform};
use serde_json::json;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "everylotbot",
    version,
    about = "Memory-bounded EveryLot Chicago bot"
)]
struct Cli {
    /// Load an explicit configuration file without changing it.
    #[arg(long, global = true)]
    env_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Args)]
struct Common {
    #[arg(long)]
    database: Option<PathBuf>,
    #[arg(short, long)]
    verbose: bool,
}
#[derive(Subcommand)]
enum Command {
    PostNext {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        dry_run: bool,
        #[arg(long,value_parser=parse_pin)]
        id: Option<String>,
        #[arg(long,default_value="all",value_parser=["all","bluesky","twitter"])]
        platform: String,
    },
    Audit(Common),
    Migrate(Common),
    Ingest {
        #[command(flatten)]
        common: Common,
        #[arg(long, default_value = "2023")]
        year: String,
        #[arg(long, default_value = "CHICAGO")]
        city: String,
        #[arg(long, default_value_t = 5000)]
        batch_size: usize,
    },
    EnrichCentroids {
        #[command(flatten)]
        common: Common,
        #[arg(long, default_value_t = 75)]
        batch_size: usize,
    },
}
fn parse_pin(value: &str) -> std::result::Result<String, String> {
    if config::valid_pin(value, 10) {
        Ok(value.to_owned())
    } else {
        Err("--id must be a 10-digit PIN10".to_owned())
    }
}
fn execute(cli: Cli) -> Result<()> {
    let mut config = Config::load(cli.env_file.as_deref())?;
    let common = match &cli.command {
        Command::PostNext { common, .. }
        | Command::Ingest { common, .. }
        | Command::EnrichCentroids { common, .. }
        | Command::Audit(common)
        | Command::Migrate(common) => common,
    };
    if let Some(path) = &common.database {
        config.database = path.clone();
    }
    match cli.command {
        Command::PostNext {
            dry_run,
            id,
            platform,
            ..
        } => {
            let platforms = match platform.as_str() {
                "bluesky" => vec![Platform::Bluesky],
                "twitter" => vec![Platform::Twitter],
                _ => config.platforms.clone(),
            };
            let result = app::run(&config, &platforms, id.as_deref(), dry_run)?;
            app::log("run_complete", result);
        }
        Command::Audit(_) => {
            let db = db::Database::open(&config.database, true)?;
            let result = db.audit(&config)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            ensure!(
                result["integrity"] == "ok",
                "Database integrity check failed"
            );
        }
        Command::Migrate(_) => {
            db::Database::open(&config.database, false)?.migrate()?;
            app::log("migration_complete", json!({}));
        }
        Command::Ingest {
            year,
            city,
            batch_size,
            ..
        } => app::log(
            "ingest_complete",
            import::ingest(&config, &year, &city, batch_size)?,
        ),
        Command::EnrichCentroids { batch_size, .. } => app::log(
            "centroid_enrichment_complete",
            import::enrich(&config, batch_size)?,
        ),
    }
    Ok(())
}
fn main() {
    let cli = Cli::parse();
    if let Err(error) = execute(cli) {
        eprintln!(
            "{}",
            json!({"event":"command_failed","message":error.to_string()})
        );
        std::process::exit(1);
    }
}
