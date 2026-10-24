//! `media:move-disk`: copy (verified, resumable), switch in one transaction, cleanup.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rblog::database::migrations::Migrator;
use rblog::models::media_move;
use rblog::services::media_disk::{self, CopyReport, Disks};
use rustcms::admin::media::entity as media;
use rustcms::audit::AuditContext;
use rustcms::storage::{LocalStorage, Storage, StorageKey};
use rustcms_testing::{TestDb, db_test};
use sea_orm::prelude::Uuid;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, PaginatorTrait, Set, Statement};
use sha2::{Digest, Sha256};

use crate::support;

/// Two local directories standing for the `local` and `s3` disks.
fn disks() -> (Disks, Arc<dyn Storage>, Arc<dyn Storage>) {
    let root = std::env::temp_dir().join(format!("rblog-move-{}", Uuid::now_v7()));
    let local: Arc<dyn Storage> = Arc::new(LocalStorage::new(&root.join("local")).unwrap());
    let s3: Arc<dyn Storage> = Arc::new(LocalStorage::new(&root.join("s3")).unwrap());
    let disks = HashMap::from([("local".to_owned(), local.clone()), ("s3".to_owned(), s3.clone())]);
    (Disks { disks, staging: root.join("staging") }, local, s3)
}

/// A media row on `local` whose file holds `content`; `sha256` overrides the recorded hash.
async fn stored(db: &DatabaseConnection, local: &dyn Storage, content: &[u8], sha256: Option<&str>) -> media::Model {
    let key = StorageKey::generate("png").unwrap();
    let file: PathBuf = std::env::temp_dir().join(format!("rblog-media-{}", Uuid::now_v7()));
    std::fs::write(&file, content).unwrap();
    local.put_file(&key, &file, "image/png").await.unwrap();
    std::fs::remove_file(&file).unwrap();
    media::ActiveModel {
        id: Set(Uuid::now_v7()),
        disk: Set("local".into()),
        key: Set(key.as_str().into()),
        name: Set("photo.png".into()),
        mime: Set("image/png".into()),
        size: Set(content.len() as i64),
        sha256: Set(sha256.map_or_else(|| hex::encode(Sha256::digest(content)), str::to_owned)),
        width: Set(None),
        height: Set(None),
        owner_id: Set(1),
        created_at: Set(std::time::SystemTime::now().into()),
        deleted_at: Set(None),
        alt: Set(None),
    }
    .insert(db)
    .await
    .unwrap()
}

db_test!(moves_files_then_switches_then_cleans_up, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, s3) = disks();
    let first = stored(db, local.as_ref(), b"first", None).await;
    let second = stored(db, local.as_ref(), b"second", None).await;

    media_disk::check_idle(db).await.unwrap();
    assert_eq!(media_disk::copy(db, &disks, "s3").await.unwrap(), CopyReport { copied: 2, skipped: 0 });
    // An interrupted run resumes: nothing is copied twice.
    assert_eq!(media_disk::copy(db, &disks, "s3").await.unwrap(), CopyReport { copied: 0, skipped: 2 });
    for row in [&first, &second] {
        assert!(s3.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
    }
    // Copied, not switched yet: rows still read from `local`.
    assert_eq!(media::Entity::find_by_id(first.id).one(db).await.unwrap().unwrap().disk, "local");

    assert_eq!(media_disk::switch(db, "s3", &AuditContext::system(Some("test"))).await.unwrap(), 2);
    assert_eq!(media::Entity::find_by_id(first.id).one(db).await.unwrap().unwrap().disk, "s3");

    assert_eq!(media_disk::cleanup(db, &disks, "s3").await.unwrap(), 2);
    assert!(!local.exists(&StorageKey::parse(&first.key).unwrap()).await.unwrap());
    assert!(s3.exists(&StorageKey::parse(&first.key).unwrap()).await.unwrap());
    assert_eq!(media_move::Entity::find().count(db).await.unwrap(), 0);
});

db_test!(refuses_a_file_that_does_not_match_its_hash, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, _) = disks();
    stored(db, local.as_ref(), b"content", Some(&"0".repeat(64))).await;

    let err = media_disk::copy(db, &disks, "s3").await.unwrap_err();
    assert!(err.to_string().contains("SHA-256"), "{err}");
    assert_eq!(media_move::Entity::find().count(db).await.unwrap(), 0);
    // Nothing copied, so nothing can be switched.
    assert!(media_disk::switch(db, "s3", &AuditContext::system(None)).await.is_err());
});

db_test!(refuses_while_deletions_are_pending, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    db.execute_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO media_deletions (id, disk, `key`, attempts, next_at) VALUES (?, 'local', '2026/10/x.png', 0, NOW(6))",
        [Uuid::now_v7().into()],
    ))
    .await
    .unwrap();
    let err = media_disk::check_idle(db).await.unwrap_err();
    assert!(err.to_string().contains("media:prune"), "{err}");
});

db_test!(cleanup_never_deletes_the_only_live_copy, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, _) = disks();
    let row = stored(db, local.as_ref(), b"live", None).await;
    media_disk::copy(db, &disks, "s3").await.unwrap();
    // Copied but not switched: the old copy is the one the application reads.
    assert!(media_disk::cleanup(db, &disks, "s3").await.is_err());
    assert!(local.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
});

db_test!(moves_back_before_cleanup, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, s3) = disks();
    let row = stored(db, local.as_ref(), b"round trip", None).await;
    media_disk::copy(db, &disks, "s3").await.unwrap();
    media_disk::switch(db, "s3", &AuditContext::system(None)).await.unwrap();

    // Something is wrong on S3: back to local, before any cleanup. The local copy is intact, so it
    // is reused rather than read from S3 again.
    assert_eq!(media_disk::copy(db, &disks, "local").await.unwrap(), CopyReport { copied: 0, skipped: 1 });
    assert_eq!(media_disk::switch(db, "local", &AuditContext::system(None)).await.unwrap(), 1);
    assert_eq!(media::Entity::find_by_id(row.id).one(db).await.unwrap().unwrap().disk, "local");
    // The copy left behind is now the S3 one.
    assert_eq!(media_disk::cleanup(db, &disks, "local").await.unwrap(), 1);
    assert!(!s3.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
    assert!(local.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
});

db_test!(cleanup_deletes_the_old_copy_of_a_deleted_media, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, s3) = disks();
    let row = stored(db, local.as_ref(), b"gone", None).await;
    media_disk::copy(db, &disks, "s3").await.unwrap();
    media_disk::switch(db, "s3", &AuditContext::system(None)).await.unwrap();
    // Deleted for good while running on S3: RustCMS deletes the S3 file only.
    s3.delete(&StorageKey::parse(&row.key).unwrap()).await.unwrap();
    media::Entity::delete_by_id(row.id).exec(db).await.unwrap();

    assert_eq!(media_disk::cleanup(db, &disks, "s3").await.unwrap(), 1);
    assert!(!local.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
});

db_test!(a_lost_target_copy_is_copied_again, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, s3) = disks();
    let row = stored(db, local.as_ref(), b"recreated bucket", None).await;
    media_disk::copy(db, &disks, "s3").await.unwrap();
    // The bucket was recreated between two runs: the journal says copied, the file is gone.
    s3.delete(&StorageKey::parse(&row.key).unwrap()).await.unwrap();

    assert_eq!(media_disk::copy(db, &disks, "s3").await.unwrap(), CopyReport { copied: 1, skipped: 0 });
    assert!(s3.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
});

db_test!(moves_back_while_the_current_disk_is_gone, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (mut disks, local, s3) = disks();
    let row = stored(db, local.as_ref(), b"s3 outage", None).await;
    media_disk::copy(db, &disks, "s3").await.unwrap();
    media_disk::switch(db, "s3", &AuditContext::system(None)).await.unwrap();
    // S3 fails: its file is unreadable, but the local copy was never cleaned up.
    s3.delete(&StorageKey::parse(&row.key).unwrap()).await.unwrap();
    disks.disks.remove("s3");
    let _ = s3;

    assert_eq!(media_disk::copy(db, &disks, "local").await.unwrap(), CopyReport { copied: 0, skipped: 1 });
    assert_eq!(media_disk::switch(db, "local", &AuditContext::system(None)).await.unwrap(), 1);
});

db_test!(cleanup_keeps_the_old_copy_when_the_new_one_is_lost, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, s3) = disks();
    let row = stored(db, local.as_ref(), b"last good copy", None).await;
    media_disk::copy(db, &disks, "s3").await.unwrap();
    media_disk::switch(db, "s3", &AuditContext::system(None)).await.unwrap();
    s3.delete(&StorageKey::parse(&row.key).unwrap()).await.unwrap();

    assert!(media_disk::cleanup(db, &disks, "s3").await.is_err());
    assert!(local.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
    assert_eq!(media_move::Entity::find().count(db).await.unwrap(), 1);
});

db_test!(
    cleanup_deletes_both_copies_of_a_media_deleted_before_the_switch,
    migrate = Migrator,
    |db: TestDb| async move {
        let app = support::app(db).await;
        let db = app.state().db();
        let (disks, local, s3) = disks();
        let row = stored(db, local.as_ref(), b"deleted on local", None).await;
        media_disk::copy(db, &disks, "s3").await.unwrap();
        // The application ran on local again and deleted it for good (its local file only).
        local.delete(&StorageKey::parse(&row.key).unwrap()).await.unwrap();
        media::Entity::delete_by_id(row.id).exec(db).await.unwrap();

        assert_eq!(media_disk::cleanup(db, &disks, "s3").await.unwrap(), 1);
        assert!(!s3.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
    }
);

db_test!(copy_again_restores_a_switched_file_lost_on_the_target, migrate = Migrator, |db: TestDb| async move {
    let app = support::app(db).await;
    let db = app.state().db();
    let (disks, local, s3) = disks();
    let row = stored(db, local.as_ref(), b"restore me", None).await;
    media_disk::copy(db, &disks, "s3").await.unwrap();
    media_disk::switch(db, "s3", &AuditContext::system(None)).await.unwrap();
    s3.delete(&StorageKey::parse(&row.key).unwrap()).await.unwrap();

    // What cleanup's error message says to run.
    assert_eq!(media_disk::copy(db, &disks, "s3").await.unwrap(), CopyReport { copied: 1, skipped: 0 });
    assert!(s3.exists(&StorageKey::parse(&row.key).unwrap()).await.unwrap());
    assert_eq!(media_disk::cleanup(db, &disks, "s3").await.unwrap(), 1);
});
