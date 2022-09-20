mod attrs;
mod blocks;
mod page_header;
#[cfg(test)]
mod tests;

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, Context};
use edn_rs::Edn;
use fxhash::FxHashMap;
use itertools::{put_back, Itertools, PutBack};
use rayon::prelude::*;
use serde::Deserialize;
use smallvec::{smallvec, SmallVec};

use crate::{
    config::DailyNotes,
    graph::{AttrList, Block, Graph, ViewType},
};

use self::blocks::LogseqRawBlock;

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
    pub properties: FxHashMap<String, serde_json::Value>,
    pub children: Vec<JsonBlock>,
    pub format: Option<BlockFormat>,
    pub content: Option<String>,
    #[serde(rename = "heading-level")]
    pub heading_level: Option<usize>,
}

pub struct PageMetadata {
    created_time: usize,
    edited_time: usize,
}

pub struct LogseqGraph {
    next_id: usize,
    root: PathBuf,
    graph: Graph,

    page_metadata: FxHashMap<String, PageMetadata>,
}

type LinesIterator<T> = PutBack<std::io::Lines<T>>;

impl LogseqGraph {
    // This is a weird way to do it since the "constructor" returns a Graph instead of a
    // LogseqGraph, but there's no reason to do otherwise in this case since we never actually want
    // to keep the LogseqGraph around.
    pub fn build(path: PathBuf, include_journals: DailyNotes) -> Result<Graph, anyhow::Error> {
        let mut lsgraph = LogseqGraph {
            next_id: 0,
            graph: Graph::new(crate::parse_string::ContentStyle::Logseq, false),
            root: path,
            page_metadata: FxHashMap::default(),
        };

        lsgraph.read_page_metadata()?;
        if include_journals != DailyNotes::Exclusive {
            lsgraph.read_page_directory("pages", false)?;
        }
        if include_journals != DailyNotes::Deny {
            lsgraph.read_page_directory("journals", true)?;
        }

        Ok(lsgraph.graph)
    }

    fn read_page_metadata(&mut self) -> Result<(), anyhow::Error> {
        let metadata_path = self.root.join("logseq").join("pages-metadata.edn");
        let source = std::fs::read_to_string(&metadata_path)
            .with_context(|| format!("Reading metadata file {metadata_path:?}"))?;
        let data = Edn::from_str(source.as_str())?;

        let blocks = match data {
            Edn::Vector(blocks) => blocks.to_vec(),
            _ => return Err(anyhow!("Unknown page-metadata format, expected list")),
        };

        self.page_metadata = blocks
            .into_iter()
            .map(|data| {
                let block_name = data
                    .get(":block/name")
                    .and_then(|v| match v {
                        Edn::Str(s) => Some(s.trim().to_string()),
                        _ => None,
                    })
                    .ok_or_else(|| anyhow!("No block name found in page-metadata block"))?;
                let created_time = data
                    .get(":block/created-at")
                    .and_then(|v| v.to_uint())
                    .unwrap_or(0);
                let edited_time = data
                    .get(":block/updated-at")
                    .and_then(|v| v.to_uint())
                    .unwrap_or(0);

                Ok((
                    block_name,
                    PageMetadata {
                        created_time,
                        edited_time,
                    },
                ))
            })
            .collect::<Result<_, anyhow::Error>>()?;

        Ok(())
    }

    fn read_page_directory(&mut self, name: &str, is_journal: bool) -> Result<(), anyhow::Error> {
        let dir = self.root.join(name);
        let files = std::fs::read_dir(&dir)
            .with_context(|| format!("{dir:?}"))?
            .map(|f| f.map(|f| f.path()))
            .collect::<Result<Vec<_>, _>>()?;

        let mut raw_pages = files
            .par_iter()
            .map(|file| read_logseq_md_file(file, is_journal).with_context(|| format!("{file:?}")))
            .collect::<Result<Vec<_>, _>>()?;

        // Can't run this step in parallel
        for page in raw_pages.iter_mut() {
            page.base_id = self.next_id;
            self.next_id += page.blocks.len() + 1;
        }

        let blocks = raw_pages
            .into_par_iter()
            .flat_map(|page| self.process_raw_page(page, is_journal))
            .collect::<Vec<_>>();

        for block in blocks {
            self.graph.add_block(block);
        }

        Ok(())
    }

    fn process_raw_page(&self, mut page: LogseqRawPage, is_journal: bool) -> Vec<Block> {
        let title = page
            .attrs
            .remove("title")
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

        let meta = title
            .as_ref()
            .map(|t| t.to_lowercase())
            .and_then(|t| self.page_metadata.get(&t));

        let page_block = Block {
            id: page.base_id,
            uid,
            containing_page: page.base_id,
            page_title: title,
            is_journal,
            string: String::new(),
            heading: 0,
            view_type,
            edit_time: meta.map(|m| m.edited_time).unwrap_or_default(),
            create_time: meta.map(|m| m.created_time).unwrap_or_default(),
            children: SmallVec::new(),

            tags,
            attrs: page.attrs,
            parent: None,
            order: 0,
        };

        let mut blocks = Vec::with_capacity(page.blocks.len() + 1);
        blocks.push(page_block);

        for (i, input) in page.blocks.into_iter().enumerate() {
            // The parent is either the index in the page, or it's the page block itself.
            let parent_block_idx = input.parent_idx.map(|i| i + 1).unwrap_or(0);
            let parent_id = parent_block_idx + page.base_id;

            let this_id = page.base_id + i + 1;
            blocks[parent_block_idx].children.push(this_id);

            let block = Block {
                id: this_id,
                uid: input.id,
                order: 0,
                parent: Some(parent_id),
                children: SmallVec::new(),
                attrs: FxHashMap::default(), // this_input.attrs,
                tags: SmallVec::new(),
                create_time: 0,
                edit_time: 0,
                view_type: input.view_type,
                string: input.contents,
                heading: input.header_level as usize,
                is_journal,
                page_title: None,
                containing_page: page.base_id,
            };

            blocks.push(block);
        }

        blocks
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LogseqRawPage {
    base_id: usize,
    attrs: FxHashMap<String, AttrList>,
    blocks: Vec<LogseqRawBlock>,
}

fn read_logseq_md_file(filename: &Path, is_journal: bool) -> Result<LogseqRawPage, anyhow::Error> {
    let file =
        File::open(filename).with_context(|| format!("Reading {}", filename.to_string_lossy()))?;
    let mut lines = put_back(BufReader::new(file).lines());
    parse_logseq_file(filename, &mut lines, is_journal)
}

fn parse_logseq_file(
    filename: &Path,
    lines: &mut LinesIterator<impl BufRead>,
    is_journal: bool,
) -> Result<LogseqRawPage, anyhow::Error> {
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
            contents: string,
            ..Default::default()
        };
        blocks.push(attrs_block);
    }

    blocks::parse_raw_blocks(&mut blocks, lines)?;

    let mut page_attrs = page_attrs_list
        .into_iter()
        .map(|(attr_name, values)| (attr_name.to_lowercase(), values))
        .collect::<FxHashMap<_, _>>();

    if !page_attrs.contains_key("title") {
        let mut title = filename
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .expect("file title");

        if is_journal {
            // Convert title from 2022_09_20 to 2022-09-20
            title = title.replace('_', "-");
        }

        page_attrs.insert(String::from("title"), smallvec![title]);
    }

    Ok(LogseqRawPage {
        base_id: 0,
        attrs: page_attrs,
        blocks,
    })
}
