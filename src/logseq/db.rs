use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use eyre::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use rusqlite_migration::{Migrations, M};

use crate::{image::Image, pic_store::PicStoreImageData};

#[derive(Debug, PartialEq, Eq)]
pub struct MetadataDbPage {
    pub filename: String,
    pub hash: Vec<u8>,
    pub created_at: i64,
    pub edited_at: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PageMatchType {
    ByFilename,
    ByHash,
}

pub struct MetadataDbPageUpdate {
    pub match_type: Option<PageMatchType>,
    pub entry: MetadataDbPage,
}

impl<'a> TryFrom<&Row<'a>> for MetadataDbPage {
    type Error = rusqlite::Error;

    fn try_from(row: &Row<'a>) -> Result<Self, Self::Error> {
        let filename = row.get(0)?;
        let hash = row.get(1)?;
        let created_at = row.get(2)?;
        let edited_at = row.get(3)?;

        Ok(MetadataDbPage {
            filename,
            hash,
            created_at,
            edited_at,
        })
    }
}

#[derive(Clone)]
pub struct MetadataDb(Arc<MetadataDbInner>);

pub struct MetadataDbInner {
    pub write_conn: Mutex<rusqlite::Connection>,
    pub root_path: PathBuf,
    read_pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

const IMAGE_DATA_VERSION: usize = 1;

impl MetadataDb {
    pub fn new(dir: PathBuf) -> Result<MetadataDb> {
        let db_path = dir.join("export-logseq-notes.sqlite3");
        let mut conn = Connection::open(&db_path)?;

        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.pragma_update(None, "synchronous", "normal")?;

        let migrations = Migrations::new(vec![
            M::up(include_str!("./migrations/0001-initial.sql")),
            M::up(include_str!("./migrations/0002-images.sql")),
        ]);

        migrations.to_latest(&mut conn)?;

        let pool_manager = r2d2_sqlite::SqliteConnectionManager::file(db_path);
        let read_pool = r2d2::Pool::builder().build(pool_manager)?;

        Ok(MetadataDb(Arc::new(MetadataDbInner {
            write_conn: Mutex::new(conn),
            read_pool,
            root_path: dir,
        })))
    }

    /// Look up a page by filename, or if the filename is not present, then look it up by hash to
    /// see if it was renamed.
    pub fn lookup_page(
        &self,
        filename: &Path,
        hash: &[u8],
    ) -> Result<Option<(PageMatchType, MetadataDbPage)>> {
        let check_path = filename
            .strip_prefix(&self.0.root_path)
            .unwrap_or(filename)
            .to_string_lossy();

        let conn = self.0.read_pool.get()?;
        let mut stmt = conn.prepare_cached(
            "SELECT filename, hash, created_at, edited_at FROM pages WHERE filename = ?",
        )?;
        let filename_row = stmt
            .query_row(params![check_path.as_ref()], |row| {
                MetadataDbPage::try_from(row)
            })
            .optional()?;
        if let Some(row) = filename_row {
            // Found a filename match
            return Ok(Some((PageMatchType::ByFilename, row)));
        }

        // If not, then look it up by hash to see if it was renamed.
        let mut stmt = conn.prepare_cached(
            "SELECT filename, hash, created_at, edited_at FROM pages WHERE hash = ?",
        )?;
        let hash_row = stmt
            .query_row(params![hash], |row| MetadataDbPage::try_from(row))
            .optional()?;

        Ok(hash_row.map(|row| (PageMatchType::ByHash, row)))
    }

    pub fn get_image(&self, image: &Image) -> Result<Option<PicStoreImageData>> {
        let conn = self.0.read_pool.get()?;
        let mut stmt = conn.prepare_cached(
            r##"SELECT data FROM images
            WHERE filename = ? AND hash = ? AND version = ?
            LIMIT 1"##,
        )?;

        let path = image.path.to_string_lossy();
        let result: Option<String> = stmt
            .query_row(
                params![
                    path.as_ref(),
                    image.hash.as_bytes().as_slice(),
                    IMAGE_DATA_VERSION
                ],
                |row| row.get(0),
            )
            .optional()?;

        let image = result
            .map(|s| serde_json::from_str::<PicStoreImageData>(&s))
            .transpose()
            .map_err(eyre::Error::from)?
            .map(|i| i.combine_2x());

        Ok(image)
    }

    pub fn add_image(&self, image: &Image, data: &PicStoreImageData) -> Result<()> {
        let conn = self.0.write_conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            r##"INSERT INTO images (filename, version, hash, data)
                VALUES (?, ?, ?, ?)
                ON CONFLICT DO UPDATE SET
                    hash=EXCLUDED.hash,
                    data=EXCLUDED.data,
                    version=EXCLUDED.version"##,
        )?;

        stmt.execute(params![
            image.path.to_string_lossy().as_ref(),
            IMAGE_DATA_VERSION,
            image.hash.as_bytes().as_slice(),
            serde_json::to_string(data)?,
        ])?;

        Ok(())
    }
}

impl Deref for MetadataDb {
    type Target = MetadataDbInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
