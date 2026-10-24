//! The demo seeder's images: small landscapes drawn here (two-colour sky, sea or field, a sun),
//! stored as PNG files in the media library with their Vietnamese alternative text, so that
//! covers, the Media page and the Content API's `alt` have something to show. No image files are
//! committed and nothing is downloaded.

use std::path::PathBuf;

use rustcms::admin::media::entity as media;
use rustcms::storage::{Storage, StorageKey};
use sea_orm::prelude::{DateTimeUtc, Uuid};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set};
use sha2::{Digest, Sha256};

/// Pixels of a demo image (16:9, about 170 KB uncompressed).
pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 180;

/// A demo image: `(file name, alternative text, sky colour, ground colour)`.
pub type DemoImage = (&'static str, &'static str, [u8; 3], [u8; 3]);

/// The demo covers, in the order of the first published demo posts.
pub const IMAGES: [DemoImage; 5] = [
    ("pho-co-ha-noi.png", "Mái ngói phố cổ Hà Nội lúc chiều muộn", [244, 196, 140], [120, 72, 52]),
    ("nuoc-dung-pho.png", "Nồi nước dùng phở bốc khói trên bếp than", [250, 226, 190], [156, 98, 54]),
    ("ca-phe-trung.png", "Tách cà phê trứng trên bàn gỗ", [236, 220, 196], [94, 64, 44]),
    ("bien-my-khe.png", "Bãi biển Mỹ Khê, Đà Nẵng, buổi sáng", [150, 200, 240], [40, 110, 170]),
    ("man-hinh-rust.png", "Màn hình máy tính với mã Rust", [60, 64, 80], [24, 26, 34]),
];

/// The library's row of `image` uploaded by `owner`: found by name among the owner's files (so a
/// second run adds nothing, and gives an alternative text to a row that has none), or stored now.
pub async fn ensure(
    txn: &DatabaseTransaction,
    storage: &dyn Storage,
    owner: i64,
    (name, alt, sky, ground): DemoImage,
) -> anyhow::Result<media::Model> {
    let found = media::Entity::find()
        .filter(media::Column::OwnerId.eq(owner))
        .filter(media::Column::Name.eq(name))
        .filter(media::Column::DeletedAt.is_null())
        .one(txn)
        .await?;
    if let Some(found) = found {
        if found.alt.as_deref().is_some_and(|a| !a.trim().is_empty()) {
            return Ok(found);
        }
        let mut described: media::ActiveModel = found.into();
        described.alt = Set(Some(alt.into()));
        return Ok(described.update(txn).await?);
    }
    let bytes = png(&landscape(sky, ground));
    let key = StorageKey::generate("png")?;
    // Staged next to the objects (a rename for the local disk), then stored.
    let staged: PathBuf = storage.staging_dir().join(format!("demo-{}", Uuid::now_v7()));
    if let Some(dir) = staged.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&staged, &bytes)?;
    let stored = storage.put_file(&key, &staged, "image/png").await;
    let _ = std::fs::remove_file(&staged);
    stored?;
    let made = media::ActiveModel {
        id: Set(Uuid::now_v7()),
        disk: Set(storage.disk().into()),
        key: Set(key.as_str().into()),
        name: Set(name.into()),
        mime: Set("image/png".into()),
        size: Set(bytes.len() as i64),
        sha256: Set(hex::encode(Sha256::digest(&bytes))),
        width: Set(Some(WIDTH as i32)),
        height: Set(Some(HEIGHT as i32)),
        alt: Set(Some(alt.into())),
        owner_id: Set(owner),
        created_at: Set(DateTimeUtc::from(std::time::SystemTime::now())),
        deleted_at: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(made)
}

/// RGB rows of a landscape: a sky fading from `sky`, a sun, and below the horizon the ground
/// fading from `ground`.
fn landscape(sky: [u8; 3], ground: [u8; 3]) -> Vec<[u8; 3]> {
    let (w, h) = (WIDTH as i64, HEIGHT as i64);
    let horizon = h * 3 / 5;
    let (sun_x, sun_y, sun_r) = (w * 7 / 10, h / 3, h / 7);
    let mix = |a: [u8; 3], b: [u8; 3], t: i64, of: i64| -> [u8; 3] {
        let channel = |i: usize| ((a[i] as i64 * (of - t) + b[i] as i64 * t) / of.max(1)) as u8;
        [channel(0), channel(1), channel(2)]
    };
    let light = |c: [u8; 3]| mix(c, [255, 255, 255], 1, 3);
    let dark = |c: [u8; 3]| mix(c, [0, 0, 0], 1, 3);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let pixel = if y < horizon {
                let (dx, dy) = (x - sun_x, y - sun_y);
                if dx * dx + dy * dy <= sun_r * sun_r { [255, 236, 170] } else { mix(light(sky), sky, y, horizon) }
            } else {
                mix(ground, dark(ground), y - horizon, h - horizon)
            };
            pixels.push(pixel);
        }
    }
    pixels
}

/// `pixels` (`WIDTH` × `HEIGHT`, RGB) as a PNG file: one IDAT of stored (uncompressed) deflate
/// blocks, which every decoder reads.
fn png(pixels: &[[u8; 3]]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(pixels.len() * 3 + HEIGHT as usize);
    for row in pixels.chunks(WIDTH as usize) {
        raw.push(0); // filter: none
        raw.extend(row.iter().flatten());
    }
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend(block);
    }
    zlib.extend(adler32(&raw).to_be_bytes());

    let mut header = Vec::with_capacity(13);
    header.extend(WIDTH.to_be_bytes());
    header.extend(HEIGHT.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]); // 8 bits, RGB, deflate, no filter method, no interlace
    let mut file = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    for (kind, data) in [(b"IHDR", header.as_slice()), (b"IDAT", zlib.as_slice()), (b"IEND", &[][..])] {
        file.extend((data.len() as u32).to_be_bytes());
        let start = file.len();
        file.extend(kind);
        file.extend(data);
        let crc = crc32(&file[start..]);
        file.extend(crc.to_be_bytes());
    }
    file
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + *byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_the_reference_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn a_demo_image_is_a_png_of_its_size() {
        let (_, _, sky, ground) = IMAGES[0];
        let file = png(&landscape(sky, ground));
        assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&file[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(file[16..20].try_into().unwrap()), WIDTH);
        assert_eq!(u32::from_be_bytes(file[20..24].try_into().unwrap()), HEIGHT);
        assert!(file.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
    }
}
