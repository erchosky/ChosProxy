//! Body blobs: optionally zstd-compressed.

use crate::StorageError;
use rusqlite::{Connection, OptionalExtension, params};

const RAW: i64 = 0;
const ZSTD: i64 = 1;
/// Bodies smaller than this are stored uncompressed.
const COMPRESS_THRESHOLD: usize = 1024;

/// Inserts a blob, compressing it when that saves space.
pub fn insert(conn: &Connection, data: &[u8]) -> Result<i64, StorageError> {
    let (encoding, stored): (i64, std::borrow::Cow<'_, [u8]>) = if data.len() >= COMPRESS_THRESHOLD
    {
        match zstd::bulk::compress(data, 3) {
            Ok(c) if c.len() < data.len() => (ZSTD, c.into()),
            _ => (RAW, data.into()),
        }
    } else {
        (RAW, data.into())
    };
    conn.prepare_cached("INSERT INTO blobs (encoding, size, data) VALUES (?1, ?2, ?3)")?
        .execute(params![encoding, data.len() as i64, stored.as_ref()])?;
    Ok(conn.last_insert_rowid())
}

/// Loads and decompresses a blob.
pub fn load(conn: &Connection, id: i64) -> Result<Option<Vec<u8>>, StorageError> {
    let row: Option<(i64, i64, Vec<u8>)> = conn
        .prepare_cached("SELECT encoding, size, data FROM blobs WHERE id = ?1")?
        .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((encoding, size, data)) = row else {
        return Ok(None);
    };
    let size = usize::try_from(size).map_err(|_| StorageError::CorruptBlob(id))?;
    match encoding {
        RAW => Ok(Some(data)),
        ZSTD => {
            let out =
                zstd::bulk::decompress(&data, size).map_err(|_| StorageError::CorruptBlob(id))?;
            if out.len() != size {
                return Err(StorageError::CorruptBlob(id));
            }
            Ok(Some(out))
        }
        _ => Err(StorageError::CorruptBlob(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_small_large_and_incompressible() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrations::migrate(&mut c).unwrap();
        let small = b"tiny".to_vec();
        let large = vec![b'a'; 100_000];
        let noise: Vec<u8> = (0..5000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        for data in [small, large, noise, Vec::new()] {
            let id = insert(&c, &data).unwrap();
            assert_eq!(load(&c, id).unwrap().unwrap(), data);
        }
        let stored: i64 = c
            .query_row(
                "SELECT length(data) FROM blobs WHERE size = 100000",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(stored < 1000, "large repetitive body should be compressed");
        assert!(load(&c, 9999).unwrap().is_none());
    }
}
