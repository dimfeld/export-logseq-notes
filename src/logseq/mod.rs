mod attrs;
mod blocks;
pub mod db;
mod page_header;
#[cfg(test)]
mod tests;

use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    str::FromStr,
    time::SystemTime,
};

use ahash::{HashMap, HashMapExt};
use edn_rs::Edn;
use eyre::{eyre, Result, WrapErr};
use itertools::{put_back, PutBack};
use rayon::prelude::*;
use rusqlite::params;
use serde::Deserialize;
use smallvec::{smallvec, SmallVec};

use self::{
    blocks::LogseqRawBlock,
    db::{MetadataDb, MetadataDbPage, MetadataDbPageUpdate, PageMatchType},
};
use crate::{
    content::BlockContent,
    graph::{AttrList, Block, BlockInclude, ParsedPage, ViewType},
    parse_string::ContentStyle,
};

#[derive(Clone, Copy, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum BlockFormat {
    Markdown,
    Unknown,
}

#[derive(Deserialize, Debug)]
pub struct JsonBlock {
    pub id: String,
    #[serde(rename = "page-name")]
    pub page_name: Option<String>,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    pub children: Vec<JsonBlock>,
    pub format: Option<BlockFormat>,
    pub content: Option<String>,
    #[serde(rename = "heading-level")]
    pub heading_level: Option<usize>,
}

pub struct PageMetadata {
    created_time: u64,
    edited_time: u64,
}

pub struct LogseqGraph {
    next_id: usize,
    root: PathBuf,

    legacy_page_metadata: HashMap<String, PageMetadata>,
}

type LinesIterator<T> = PutBack<std::io::Lines<T>>;

impl LogseqGraph {
    // This is a weird way to do it since the "constructor" returns a Graph instead of a
    // LogseqGraph, but there's no reason to do otherwise in this case since we never actually want
    // to keep the LogseqGraph around and this API isn't exposed to the outside world.
    pub fn build(
        path: PathBuf,
        metadata_db: Option<MetadataDb>,
    ) -> Result<(ContentStyle, bool, Vec<ParsedPage>)> {
        let mut lsgraph = LogseqGraph {
            next_id: 0,
            root: path,
            legacy_page_metadata: HashMap::default(),
        };

        lsgraph.read_legacy_page_metadata()?;
        let mut pages = lsgraph.read_page_directory("pages", &metadata_db, false)?;
        let journals = lsgraph.read_page_directory("journals", &metadata_db, true)?;

        pages.extend(journals.into_iter());
        Ok((ContentStyle::Logseq, false, pages))
    }

    /// Read the pages-metadata.edn file. Logseq does not use this anymore, but if it exists, we read
    /// it for the initial population of the metadata database.
    fn read_legacy_page_metadata(&mut self) -> Result<()> {
        let metadata_path = self.root.join("logseq").join("pages-metadata.edn");
        let source = match std::fs::read_to_string(metadata_path) {
            Ok(data) => data,
            Err(_) => {
                // pages-metadata.edn no longer exists with newer versions of Logseq, so that's
                // fine.
                self.legacy_page_metadata = HashMap::default();
                return Ok(());
            }
        };

        let data = Edn::from_str(source.as_str())?;

        let blocks = match data {
            Edn::Vector(blocks) => blocks.to_vec(),
            _ => return Err(eyre!("Unknown page-metadata format, expected list")),
        };

        self.legacy_page_metadata = blocks
            .into_iter()
            .map(|data| {
                let block_name = data
                    .get(":block/name")
                    .and_then(|v| match v {
                        Edn::Str(s) => Some(s.trim().to_string()),
                        _ => None,
                    })
                    .ok_or_else(|| eyre!("No block name found in page-metadata block"))?;
                let created_time = data
                    .get(":block/created-at")
                    .and_then(|v| v.to_uint())
                    .unwrap_or(0) as u64;
                let edited_time = data
                    .get(":block/updated-at")
                    .and_then(|v| v.to_uint())
                    .unwrap_or(0) as u64;

                Ok((
                    block_name,
                    PageMetadata {
                        created_time,
                        edited_time,
                    },
                ))
            })
            .collect::<Result<_>>()?;

        Ok(())
    }

    fn read_page_directory(
        &mut self,
        name: &str,
        metadata_db: &Option<MetadataDb>,
        is_journal: bool,
    ) -> Result<Vec<ParsedPage>> {
        let dir = self.root.join(name);
        let files = std::fs::read_dir(&dir)
            .with_context(|| format!("{dir:?}"))?
            .map(|f| f.map(|f| f.path()))
            .collect::<Result<Vec<_>, _>>()?;

        let mut raw_pages = files
            .par_iter()
            .filter(|file| file.extension().map(|ext| ext == "md").unwrap_or(false))
            .map(|file| {
                read_logseq_md_file(file, metadata_db, is_journal)
                    .with_context(|| format!("{file:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Can't run this step in parallel
        for page in raw_pages.iter_mut() {
            page.base_id = self.next_id;
            self.next_id += page.blocks.len() + 1;
        }

        let pages = raw_pages
            .into_par_iter()
            .map(|page| self.process_raw_page(page, is_journal))
            .collect::<Vec<_>>();

        let output = if let Some(metadata_db) = metadata_db {
            let mut output = Vec::with_capacity(pages.len());
            let mut conn = metadata_db.write_conn.lock().unwrap();
            let tx = conn.transaction()?;

            {
                let mut insert_stmt = tx.prepare_cached(
                r##"INSERT INTO pages (filename, hash, created_at, edited_at) VALUES (?, ?, ?, ?)"##)?;

                let mut update_by_hash_stmt = tx.prepare_cached(
                    r##"UPDATE pages
                    SET filename = ?, edited_at = ?
                    WHERE hash = ?"##,
                )?;

                let mut update_by_filename_stmt = tx.prepare_cached(
                    r##"UPDATE pages
                    SET hash = ?, edited_at = ?
                    WHERE filename = ?"##,
                )?;

                for (db_meta, page) in pages {
                    output.push(page);

                    match db_meta {
                        Some(MetadataDbPageUpdate {
                            match_type: Some(PageMatchType::ByFilename),
                            entry,
                        }) => {
                            update_by_filename_stmt.execute(params![
                                &entry.hash,
                                entry.edited_at,
                                entry.filename
                            ])?;
                        }
                        Some(MetadataDbPageUpdate {
                            match_type: Some(PageMatchType::ByHash),
                            entry,
                        }) => {
                            update_by_hash_stmt.execute(params![
                                entry.filename,
                                entry.edited_at,
                                &entry.hash
                            ])?;
                        }
                        Some(MetadataDbPageUpdate {
                            match_type: None,
                            entry,
                        }) => {
                            insert_stmt.execute(params![
                                &entry.filename,
                                &entry.hash,
                                entry.created_at,
                                entry.edited_at
                            ])?;
                        }
                        None => {}
                    }
                }
            }

            tx.commit()?;
            output
        } else {
            pages.into_iter().map(|(_, page)| page).collect()
        };

        Ok(output)
    }

    fn resolve_metadata(
        &self,
        page: &mut LogseqRawPage,
        title: &Option<String>,
        is_journal: bool,
    ) -> (Option<MetadataDbPageUpdate>, u64, u64) {
        let legacy_meta = title
            .as_ref()
            .map(|t| t.to_lowercase())
            .and_then(|t| self.legacy_page_metadata.get(&t));

        let (default_time, fs_create_time) = if is_journal {
            let mut i = title.as_deref().unwrap_or_default().splitn(3, '-');
            let y = i.next().map(|x| x.parse::<i32>());
            let m = i.next().map(|x| x.parse::<u32>());
            let d = i.next().map(|x| x.parse::<u32>());

            let default_time = match (y, m, d) {
                (Some(Ok(y)), Some(Ok(m)), Some(Ok(d))) => chrono::NaiveDate::from_ymd_opt(y, m, d)
                    .map(|d| d.and_hms(0, 0, 0).timestamp_millis() as u64)
                    .unwrap_or_default(),
                _ => 0,
            };

            // For journals we always return the journal's date as the create date
            (default_time, default_time)
        } else {
            let default_time = 0;
            let create_time = legacy_meta
                .map(|m| m.created_time)
                .or(page.created_time)
                .unwrap_or(default_time);
            (default_time, create_time)
        };

        // If the updated time is the same as the created time, and we also have metadata for the page,
        // then just use the metadata since it was probably more correct. This is kind of a gross
        // heuristic but actually does help with some legacy pages.
        let fs_edit_time = match (
            page.created_time.is_some(),
            page.updated_time == page.created_time,
            legacy_meta,
        ) {
            (true, true, Some(meta)) => std::cmp::max(meta.edited_time, fs_create_time),
            _ => page
                .updated_time
                .or_else(|| legacy_meta.map(|m| m.edited_time))
                .unwrap_or(default_time),
        };

        let (db_update, created_time, updated_time) = match page.metadata_entry.take() {
            Some((match_type, meta)) => {
                if meta.hash == page.hash {
                    let created_at = meta.created_at as u64;
                    let edited_at = meta.edited_at as u64;
                    let db_update = match match_type {
                        // We matched on hash but not on filename, so the file was renamed. Update the
                        // filename.
                        PageMatchType::ByHash => Some(MetadataDbPageUpdate {
                            match_type: Some(match_type),
                            entry: meta,
                        }),
                        // The filename didn't change, so there's nothing to do.
                        PageMatchType::ByFilename => None,
                    };

                    // The hash didn't change, so we continue to use the timestamps from the
                    // database.
                    (db_update, created_at, edited_at)
                } else {
                    // The hash changed, so we use the edited timestamp from the file. The created
                    // timestamp stays the same as what's in the database.
                    (
                        Some(MetadataDbPageUpdate {
                            match_type: Some(match_type),
                            entry: MetadataDbPage {
                                filename: meta.filename,
                                hash: page.hash.to_vec(),
                                created_at: meta.created_at,
                                edited_at: page.updated_time.unwrap_or(0) as i64,
                            },
                        }),
                        meta.created_at as u64,
                        fs_edit_time,
                    )
                }
            }
            None => {
                // This is a new entry, so use the filesystem timestamps.
                let filename = page
                    .path
                    .strip_prefix(&self.root)
                    .unwrap_or(&page.path)
                    .to_string_lossy()
                    .into_owned();

                let db_update = MetadataDbPageUpdate {
                    match_type: None,
                    entry: MetadataDbPage {
                        filename,
                        hash: Vec::from_iter(page.hash),
                        created_at: fs_create_time as i64,
                        edited_at: fs_edit_time as i64,
                    },
                };

                (Some(db_update), fs_create_time, fs_edit_time)
            }
        };

        (db_update, created_time, updated_time)
    }

    fn process_raw_page(
        &self,
        mut page: LogseqRawPage,
        is_journal: bool,
    ) -> (Option<MetadataDbPageUpdate>, ParsedPage) {
        let title = page
            .attrs
            .remove("title")
            .map(|mut values| values.remove(0));
        let original_title = page
            .attrs
            .remove("original_title")
            .map(|mut values| values.remove(0));

        let uid = page
            .attrs
            .remove("id")
            .map(|mut values| values.remove(0))
            .unwrap_or_default(); // TODO probably want to generate a uuid
        let tags = page.attrs.get("tags").cloned().unwrap_or_default();
        let view_type = page
            .attrs
            .get("view-mode")
            .and_then(|values| values.get(0))
            .map(ViewType::from)
            .unwrap_or_default();

        let (db_meta, create_time, edit_time) =
            self.resolve_metadata(&mut page, &title, is_journal);

        let page_block = Block {
            id: page.base_id,
            uid,
            include_type: BlockInclude::IfChildrenPresent,
            containing_page: page.base_id,
            page_title: title,
            original_title,
            is_journal,
            contents: BlockContent::new_empty(ContentStyle::Logseq),
            heading: 0,
            view_type,
            this_block_list_type: crate::graph::ListType::Default,
            create_time,
            edit_time,
            children: SmallVec::new(),

            extra_classes: Vec::new(),
            content_element: None,
            wrapper_element: None,

            tags,
            attrs: page.attrs,
            parent: None,
            order: 0,
        };

        let mut blocks = HashMap::with_capacity(page.blocks.len() + 1);
        let root_block = page_block.id;
        blocks.insert(page_block.id, page_block);

        for (i, input) in page.blocks.into_iter().enumerate() {
            // The parent is either the index in the page, or it's the page block itself.
            let parent_block_idx = input.parent_idx.map(|i| i + 1).unwrap_or(0);
            let parent_id = parent_block_idx + page.base_id;

            let this_id = page.base_id + i + 1;
            blocks.get_mut(&parent_id).unwrap().children.push(this_id);

            let block = Block {
                id: this_id,
                uid: input.id,
                include_type: BlockInclude::default(),
                order: 0,
                parent: Some(parent_id),
                children: SmallVec::new(),
                attrs: input.attrs,
                tags: input.tags,
                create_time: 0,
                edit_time: 0,
                view_type: input.view_type,
                this_block_list_type: input.this_block_list_type,
                contents: input.contents,
                heading: input.header_level as usize,
                is_journal,
                page_title: None,
                original_title: None,
                containing_page: page.base_id,
                extra_classes: Vec::new(),
                content_element: None,
                wrapper_element: None,
            };

            blocks.insert(block.id, block);
        }

        (
            db_meta,
            ParsedPage {
                root_block,
                blocks,
                path: page.path,
            },
        )
    }
}

#[derive(Debug, Eq)]
struct LogseqRawPage {
    path: PathBuf,
    base_id: usize,
    attrs: HashMap<String, AttrList>,
    blocks: Vec<LogseqRawBlock>,
    created_time: Option<u64>,
    updated_time: Option<u64>,
    metadata_entry: Option<(PageMatchType, MetadataDbPage)>,
    hash: [u8; 32],
}

impl PartialEq for LogseqRawPage {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.base_id == other.base_id
            && self.attrs == other.attrs
            && self.blocks == other.blocks
            && self.created_time == other.created_time
            && self.updated_time == other.updated_time
            && self.metadata_entry == other.metadata_entry
            && self.hash == other.hash
    }
}

fn read_logseq_md_file(
    filename: &Path,
    metadata_db: &Option<MetadataDb>,
    is_journal: bool,
) -> Result<LogseqRawPage> {
    let mut file =
        File::open(filename).with_context(|| format!("Reading {}", filename.display()))?;
    let meta = file
        .metadata()
        .with_context(|| format!("Reading {}", filename.display()))?;

    let updated = meta
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .ok();
    let created = meta
        .created()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .ok();

    let size = meta.len();
    let mut contents = Vec::with_capacity(size as usize);
    file.read_to_end(&mut contents)?;

    let hash = blake3::hash(&contents);

    let metadata_entry = metadata_db
        .as_ref()
        .and_then(|m| m.lookup_page(filename, hash.as_bytes()).transpose())
        .transpose()?;

    let mut lines = put_back(BufReader::new(std::io::Cursor::new(contents)).lines());
    let (attrs, blocks) = parse_logseq_file(filename, &mut lines, is_journal)?;
    Ok(LogseqRawPage {
        path: PathBuf::from(filename),
        base_id: 0,
        attrs,
        blocks,
        created_time: created,
        updated_time: updated,
        metadata_entry,
        hash: hash.into(),
    })
}

fn parse_logseq_file(
    filename: &Path,
    lines: &mut LinesIterator<impl BufRead>,
    is_journal: bool,
) -> Result<(HashMap<String, AttrList>, Vec<LogseqRawBlock>)> {
    let page_attrs_list = page_header::parse_page_header(lines)?;

    // Create a block containing the page header attributes so that it will show up in the output
    let attrs_block_contents = page_attrs_list
        .iter()
        .filter(|(attr_name, _)| !matches!(attr_name.as_str(), "id" | "title"))
        .map(|(attr_name, attr_values)| {
            let values = attr_values.join(", ");
            format!("{attr_name}:: {values}")
        })
        .collect::<Vec<_>>();

    let mut blocks = Vec::new();

    for string in attrs_block_contents {
        let attrs_block = LogseqRawBlock {
            contents: BlockContent::new_parsed(ContentStyle::Logseq, string)?,
            ..Default::default()
        };
        blocks.push(attrs_block);
    }

    blocks::parse_raw_blocks(&mut blocks, lines)?;

    let mut page_attrs = page_attrs_list
        .into_iter()
        .map(|(attr_name, values)| (attr_name.to_lowercase(), values))
        .collect::<HashMap<_, _>>();

    let has_title_attr = page_attrs.contains_key("title");
    let orig_title_key = if has_title_attr {
        "original_title"
    } else {
        "title"
    };

    let mut title = filename
        .file_stem()
        .map(|s| {
            let s = s.to_string_lossy().into_owned();
            urlencoding::decode(&s).map(|s| s.into_owned()).unwrap_or(s)
        })
        .expect("file title");

    if is_journal {
        // Convert title from 2022_09_20 to 2022-09-20
        title = title.replace('_', "-");
    }

    page_attrs.insert(String::from(orig_title_key), smallvec![title]);

    Ok((page_attrs, blocks))
}
