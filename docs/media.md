# Media storage

RBlog keeps uploaded media (images from the editor and covers, PDF files) on **one disk**, chosen
when deploying in `config/storage.toml` (RustCMS `[storage]`, see the RustCMS guide
`digging-deeper/storage.md`):

| `disk` | Where | Settings |
|---|---|---|
| `local` (default) | `root` (`storage/app`) on the server | back it up; share it (one volume) between instances |
| `s3` | a **private** bucket on AWS S3, Cloudflare R2, MinIO, … | `[s3] bucket`, `region`, `endpoint` (non-AWS); `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` |

Every key can come from the environment (`RUSTCMS_STORAGE__DISK=s3`,
`RUSTCMS_STORAGE__S3__BUCKET=…`). The binary always supports both (Cargo feature `s3`).

Limits: `max_upload_bytes` (10 MiB), `allowed_types` (PNG, JPEG, GIF, WebP, PDF; SVG is never
accepted). Readers never fetch from the bucket directly: public images are served by RBlog's
Content API (docs/api.md), which checks that they belong to published content.

## Trying S3 locally

```sh
docker compose --profile s3 up -d     # S3Mock with a bucket `rblog` on http://localhost:9090
export RUSTCMS_STORAGE__DISK=s3 RUSTCMS_STORAGE__S3__BUCKET=rblog RUSTCMS_STORAGE__S3__REGION=us-east-1 \
  RUSTCMS_STORAGE__S3__ENDPOINT=http://localhost:9090 RUSTCMS_STORAGE__S3__ALLOW_HTTP=true \
  AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test
rustcms serve
```

## Moving an existing site to another disk

RustCMS reads every file from the disk it is bound to, so a move happens **with the application
stopped** (a maintenance window), in three commands:

```sh
# 1. Stop the application. Configure both disks in [storage] (the S3 bucket and credentials too).
rblog media:prune                     # settles pending uploads and deletions
rblog media:move-disk --to s3         # copies every file (SHA-256 checked), then switches the rows
# 2. Set disk = "s3" (or RUSTCMS_STORAGE__DISK=s3) and start the application. Check the images.
# 3. Later, once all is well:
rblog media:move-disk --to s3 --cleanup   # deletes the old copies
```

- The copy is **resumable**: a file whose copy on the target is already intact (present, SHA-256
  matching) is not read again, so running the command again continues where it stopped. The
  journal (`media_moves`) is never trusted alone: a missing or damaged target copy is redone.
- **Moving back** before `--cleanup` (`--to local` after `--to s3`) reuses the intact old copies,
  so it works even when the disk you are leaving is failing.
- A file whose content does not match its recorded SHA-256 stops the move; nothing is switched.
- The rows are switched in **one transaction**, recorded in the audit log (`media.disk_moved`),
  and only once every file is copied. Uploads made meanwhile make it refuse: copy again.
- `--cleanup` never deletes a file the application still reads from the old disk.
