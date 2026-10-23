//! Moving the media library to another disk (`local` ↔ `s3`), for `media:move-disk`.
//!
//! RustCMS binds one disk at a time, and a file whose `media.disk` is not the bound disk cannot
//! be downloaded. So a move runs while the application is stopped, in three steps:
//!
//! 1. [`copy`]: every file not yet on the target is read from its disk, checked against its
//!    recorded SHA-256, written to the target and journaled in `media_moves`. An interrupted
//!    copy resumes where it stopped.
//! 2. [`switch`]: once nothing is left to copy, every `media.disk` is set to the target in one
//!    transaction (audited). Then `[storage] disk` is changed and the application restarted.
//! 3. [`cleanup`]: later, the old copies are deleted and the journal emptied.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use rustcms::admin::media::entity as media;
use rustcms::audit::{self, AuditContext, AuditEntry};
use rustcms::storage::{Storage, StorageKey};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, Statement,
};
use sha2::{Digest, Sha256};

use crate::models::media_move;

/// Files copied per batch.
const BATCH: u64 = 100;

/// The disks a move reads from and writes to, by name (`local`, `s3`).
pub struct Disks {
    pub disks: HashMap<String, Arc<dyn Storage>>,
    /// Where a file is staged between reading and writing it (`put_file` takes a path).
    pub staging: PathBuf,
}

impl Disks {
    fn get(&self, name: &str) -> anyhow::Result<&Arc<dyn Storage>> {
        self.disks.get(name).with_context(|| format!("disk `{name}` is not configured in [storage]"))
    }
}

/// What [`copy`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CopyReport {
    pub copied: u64,
    /// Already copied by an earlier, interrupted run.
    pub skipped: u64,
}

/// Refuses to move while uploads or deletions are pending: their files are on the bound disk
/// and the move would leave them behind. `rblog media:prune` settles both.
pub async fn check_idle(db: &DatabaseConnection) -> anyhow::Result<()> {
    for table in ["media_uploads", "media_deletions"] {
        let row = db
            .query_one_raw(Statement::from_string(
                db.get_database_backend(),
                format!("SELECT COUNT(*) AS n FROM {table}"),
            ))
            .await?
            .context("COUNT returned no row")?;
        let pending: i64 = row.try_get("", "n")?;
        if pending > 0 {
            bail!("{pending} pending row(s) in {table}: stop the application, run `rblog media:prune`, then retry");
        }
    }
    Ok(())
}

/// Copies every media file not on `to` yet, verifying each one's SHA-256 (trashed media too).
///
/// A file whose copy on `to` is already intact is not read from its disk again: an interrupted
/// copy resumes, and moving back before cleanup works even when the current disk is failing.
/// The journal alone is never trusted: a missing or damaged copy on `to` is copied again.
pub async fn copy(db: &DatabaseConnection, disks: &Disks, to: &str) -> anyhow::Result<CopyReport> {
    let target = disks.get(to)?;
    std::fs::create_dir_all(&disks.staging)
        .with_context(|| format!("creating the staging directory {}", disks.staging.display()))?;
    let mut report = CopyReport::default();
    let mut after = None;
    loop {
        let mut query = media::Entity::find().filter(media::Column::Disk.ne(to)).order_by_asc(media::Column::Id);
        if let Some(id) = after {
            query = query.filter(media::Column::Id.gt(id));
        }
        let rows = query.limit(BATCH).all(db).await?;
        let Some(last) = rows.last() else { break };
        after = Some(last.id);
        for row in rows {
            if intact(target.as_ref(), &row).await? {
                report.skipped += 1;
            } else {
                copy_one(disks.get(&row.disk)?.as_ref(), target.as_ref(), &disks.staging, &row).await?;
                report.copied += 1;
            }
            journal(db, &row, to).await?;
        }
    }
    // Already switched but not cleaned up: a copy on `to` lost since (a recreated bucket) is
    // restored from the old copy the journal still lists.
    let mut after = None;
    loop {
        let mut query = media_move::Entity::find().filter(media_move::Column::ToDisk.eq(to));
        if let Some(id) = after {
            query = query.filter(media_move::Column::MediaId.gt(id));
        }
        let moves = query.order_by_asc(media_move::Column::MediaId).limit(BATCH).all(db).await?;
        let Some(last) = moves.last() else { break };
        after = Some(last.media_id);
        for moved in moves {
            let Some(row) = media::Entity::find_by_id(moved.media_id).one(db).await? else { continue };
            if row.disk != to || intact(target.as_ref(), &row).await? {
                continue;
            }
            copy_one(disks.get(&moved.from_disk)?.as_ref(), target.as_ref(), &disks.staging, &row).await?;
            report.copied += 1;
        }
    }
    Ok(report)
}

/// Records that `row`'s file is on `to`, its old copy on `row.disk`. A journal row of an earlier
/// move the other way (not cleaned up yet) is replaced: the copy left behind is now on `row.disk`.
async fn journal(db: &DatabaseConnection, row: &media::Model, to: &str) -> anyhow::Result<()> {
    let existing = media_move::Entity::find_by_id(row.id).one(db).await?;
    if existing.as_ref().is_some_and(|m| m.to_disk == to && m.from_disk == row.disk) {
        return Ok(());
    }
    let model = media_move::ActiveModel {
        media_id: Set(row.id),
        from_disk: Set(row.disk.clone()),
        key: Set(row.key.clone()),
        to_disk: Set(to.to_owned()),
        copied_at: Set(chrono_now()),
    };
    if existing.is_some() {
        model.update(db).await?;
    } else {
        model.insert(db).await?;
    }
    Ok(())
}

/// Reads the whole file `row` names on `storage` and checks it against the recorded SHA-256.
async fn read_verified(storage: &dyn Storage, row: &media::Model) -> anyhow::Result<Option<Vec<u8>>> {
    let key = StorageKey::parse(&row.key)?;
    let (body, len) = storage.open(&key).await?;
    let limit = usize::try_from(len).unwrap_or(usize::MAX).saturating_add(1);
    let bytes = rustcms::axum::body::to_bytes(body, limit).await?;
    Ok((hex::encode(Sha256::digest(&bytes)) == row.sha256).then(|| bytes.to_vec()))
}

/// Whether `target` already holds an intact copy of `row`'s file.
async fn intact(target: &dyn Storage, row: &media::Model) -> anyhow::Result<bool> {
    if !target.exists(&StorageKey::parse(&row.key)?).await? {
        return Ok(false);
    }
    // A copy that cannot be read or does not match is copied again.
    Ok(read_verified(target, row).await.ok().flatten().is_some())
}

async fn copy_one(
    source: &dyn Storage,
    target: &dyn Storage,
    staging: &Path,
    row: &media::Model,
) -> anyhow::Result<()> {
    let Some(bytes) =
        read_verified(source, row).await.with_context(|| format!("reading {} ({})", row.key, row.disk))?
    else {
        bail!("{} ({}) does not match its recorded SHA-256: nothing was moved for it", row.key, row.disk);
    };
    let key = StorageKey::parse(&row.key)?;
    let staged = staging.join(format!("{}.move", row.id));
    std::fs::write(&staged, &bytes).with_context(|| format!("staging {}", staged.display()))?;
    let written = target.put_file(&key, &staged, &row.mime).await;
    let _ = std::fs::remove_file(&staged);
    written.with_context(|| format!("writing {}", row.key))?;
    if !intact(target, row).await? {
        bail!("{} is missing or damaged on the target after writing it", row.key);
    }
    Ok(())
}

/// Points every media row at `to`, in one transaction, once every file is copied there.
/// Returns the number of rows switched.
pub async fn switch(db: &DatabaseConnection, to: &str, ctx: &AuditContext) -> anyhow::Result<u64> {
    let txn = rustcms::db::begin_write(db).await?;
    // Locked: a row inserted or changed meanwhile (an upload that should not happen while the
    // application is stopped) waits, and only the rows checked here are switched.
    let pending = media::Entity::find().filter(media::Column::Disk.ne(to)).lock_exclusive().all(&txn).await?;
    let copied: HashSet<_> = media_move::Entity::find()
        .filter(media_move::Column::ToDisk.eq(to))
        .all(&txn)
        .await?
        .into_iter()
        .map(|m| m.media_id)
        .collect();
    let missing = pending.iter().filter(|r| !copied.contains(&r.id)).count();
    if missing > 0 {
        bail!("{missing} file(s) are not copied to `{to}` yet: run the copy again (new uploads arrived?)");
    }
    let mut switched = 0;
    for ids in pending.iter().map(|r| r.id).collect::<Vec<_>>().chunks(BATCH as usize) {
        switched += media::Entity::update_many()
            .col_expr(media::Column::Disk, sea_orm::sea_query::Expr::value(to))
            .filter(media::Column::Id.is_in(ids.to_vec()))
            .exec(&txn)
            .await?
            .rows_affected;
    }
    audit::record(
        &txn,
        ctx,
        AuditEntry::new("media.disk_moved", "media").meta(serde_json::json!({ "to": to, "files": switched })),
    )
    .await?;
    txn.commit().await?;
    Ok(switched)
}

/// Deletes the old copies of files now on `to`, and their journal rows.
pub async fn cleanup(db: &DatabaseConnection, disks: &Disks, to: &str) -> anyhow::Result<u64> {
    let mut deleted = 0;
    loop {
        let moves = media_move::Entity::find()
            .filter(media_move::Column::ToDisk.eq(to))
            .order_by_asc(media_move::Column::MediaId)
            .limit(BATCH)
            .all(db)
            .await?;
        if moves.is_empty() {
            break;
        }
        for moved in moves {
            let row = media::Entity::find_by_id(moved.media_id).one(db).await?;
            let key = StorageKey::parse(&moved.key)?;
            match row {
                // Still read from the old disk: never delete its only live copy.
                Some(row) if row.disk != to => {
                    bail!("{} is not switched to `{to}` yet: run without --cleanup", row.key)
                }
                // The old copy goes only once the one the application reads is checked.
                Some(row) => {
                    if !intact(disks.get(to)?.as_ref(), &row).await? {
                        bail!(
                            "{} is missing or damaged on `{to}`: its copy on `{}` is kept; run `media:move-disk --to {to}` \
                             to copy it again",
                            row.key,
                            moved.from_disk
                        );
                    }
                    deleted += delete_if_present(disks.get(&moved.from_disk)?.as_ref(), &key).await?;
                }
                // Deleted for good since: RustCMS removed the file on the disk it was bound to
                // then, which may be either. Nothing else knows about the other copy.
                None => {
                    for disk in [&moved.from_disk, &moved.to_disk] {
                        deleted += delete_if_present(disks.get(disk)?.as_ref(), &key).await?;
                    }
                }
            }
            media_move::Entity::delete_by_id(moved.media_id).exec(db).await?;
        }
    }
    Ok(deleted)
}

async fn delete_if_present(storage: &dyn Storage, key: &StorageKey) -> anyhow::Result<u64> {
    if !storage.exists(key).await? {
        return Ok(0);
    }
    storage.delete(key).await?;
    Ok(1)
}

fn chrono_now() -> sea_orm::prelude::DateTimeUtc {
    sea_orm::prelude::DateTimeUtc::from(std::time::SystemTime::now())
}
