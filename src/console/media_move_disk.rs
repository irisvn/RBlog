//! `media:move-disk` (registered in `providers/app_service_provider.rs`): moves the media
//! library to another disk. See `services::media_disk` and docs/media.md.

use std::collections::HashMap;

use rustcms::audit::AuditContext;
use rustcms::clap::{Arg, ArgAction, ArgMatches, Command};
use rustcms::storage::{self, StorageConfig};
use rustcms::{AppCommand, AppState, async_trait};
use sea_orm::{EntityTrait, QuerySelect};

use crate::services::media_disk::{self, Disks};

pub struct MediaMoveDisk;

#[async_trait]
impl AppCommand for MediaMoveDisk {
    fn definition(&self) -> Command {
        Command::new("media:move-disk")
            .about("Move the media library to another disk (local or s3), with the application stopped")
            .arg(Arg::new("to").long("to").required(true).value_parser(["local", "s3"]).help("The target disk"))
            .arg(
                Arg::new("cleanup")
                    .long("cleanup")
                    .action(ArgAction::SetTrue)
                    .help("Delete the old copies once the application runs on the target disk"),
            )
    }

    async fn run(&self, state: &AppState, args: &ArgMatches) -> anyhow::Result<()> {
        let to = args.get_one::<String>("to").expect("required");
        let db = state.db();
        let config: StorageConfig = state.config().section("storage")?;
        let disks = disks(db, &config, to).await?;

        if args.get_flag("cleanup") {
            let deleted = media_disk::cleanup(db, &disks, to).await?;
            println!("Deleted {deleted} old cop(y/ies); every file is on `{to}` only.");
            return Ok(());
        }

        media_disk::check_idle(db).await?;
        let report = media_disk::copy(db, &disks, to).await?;
        println!("Copied {} file(s) to `{to}` ({} already copied before).", report.copied, report.skipped);
        let switched = media_disk::switch(db, to, &AuditContext::system(Some("media:move-disk"))).await?;
        println!("Switched {switched} media row(s) to `{to}`.");
        if config.disk != to.as_str() {
            println!(
                "Now set `disk = \"{to}\"` in config/storage.toml (or RUSTCMS_STORAGE__DISK={to}) and start the \
                 application. Later, `rblog media:move-disk --to {to} --cleanup` deletes the old copies."
            );
        }
        Ok(())
    }
}

/// The target disk and every disk a media row is on, each built from `[storage]` with its name.
async fn disks(db: &sea_orm::DatabaseConnection, config: &StorageConfig, to: &str) -> anyhow::Result<Disks> {
    let mut names: Vec<String> = rustcms::admin::media::entity::Entity::find()
        .select_only()
        .column(rustcms::admin::media::entity::Column::Disk)
        .distinct()
        .into_tuple()
        .all(db)
        .await?;
    names.extend(
        crate::models::media_move::Entity::find()
            .select_only()
            .column(crate::models::media_move::Column::FromDisk)
            .distinct()
            .into_tuple::<String>()
            .all(db)
            .await?,
    );
    names.push(to.to_owned());
    let mut disks = HashMap::new();
    for name in names {
        if disks.contains_key(&name) {
            continue;
        }
        let disk = storage::connect(&StorageConfig { disk: name.clone(), ..config.clone() })
            .map_err(|e| anyhow::anyhow!("disk `{name}`: {e}"))?;
        disks.insert(name, disk);
    }
    Ok(Disks { disks, staging: config.root.join(".move-staging") })
}
