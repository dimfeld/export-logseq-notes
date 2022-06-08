mod attrs;
mod blocks;
mod page_header;

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, Context};
use edn_rs::Edn;
use fxhash::FxHashMap;
use itertools::{put_back, PutBack};
use rayon::prelude::*;
use serde::Deserialize;
use smallvec::{smallvec, SmallVec};

use crate::graph::{AttrList, Block, Graph, ViewType};

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
    pub fn build(path: PathBuf) -> Result<Graph, anyhow::Error> {
        let mut lsgraph = LogseqGraph {
            next_id: 0,
            graph: Graph::new(crate::parse_string::ContentStyle::Logseq, false),
            root: path,
            page_metadata: FxHashMap::default(),
        };

        lsgraph.read_page_metadata()?;
        lsgraph.read_page_directory("pages", false)?;
        lsgraph.read_page_directory("journals", true)?;

        // for block in file.blocks {
        //     lsgraph.add_block_and_children(None, None, &block)?;
        // }

        Ok(lsgraph.graph)
    }

    fn read_page_metadata(&mut self) -> Result<(), anyhow::Error> {
        let metadata_path = self.root.join("logseq").join("pages-metadata.edn");
        let source = std::fs::read_to_string(metadata_path)?;
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
                    .map(|v| v.to_string())
                    .ok_or_else(|| anyhow!("No block name found in page-metadata block"))?;
                let created_time = data
                    .get(":block/created-at")
                    .and_then(|v| v.to_uint())
                    .unwrap_or(0);
                let edited_time = data
                    .get(":block/edited-at")
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
        let files = std::fs::read_dir(&dir)?
            .map(|f| f.map(|f| f.path()))
            .collect::<Result<Vec<_>, _>>()?;

        let mut raw_pages = files
            .par_iter()
            .map(|file| read_logseq_md_file(file))
            .collect::<Result<Vec<_>, _>>()?;

        // Can't run this step in parallel
        for page in raw_pages.iter_mut() {
            page.base_id = self.next_id;
            self.next_id += page.blocks.len()
        }

        let pages = raw_pages
            .into_par_iter()
            .map(|page| self.process_raw_page(page, is_journal))
            .collect::<Vec<_>>();

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
        let tags = page.attrs.remove("tags").unwrap_or_default();
        let view_type = match page
            .attrs
            .get("view-mode")
            .and_then(|values| values.get(0))
            .map(|s| s.as_str())
        {
            Some("document") => ViewType::Document,
            Some("numbered") => ViewType::Numbered,
            _ => ViewType::Bullet,
        };

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
            // TODO need to get attrs for each block
            // let view_type = match input
            //     .attrs
            //     .get("view-mode")
            //     .and_then(|values| values.get(0))
            //     .map(|s| s.as_str())
            // {
            //     Some("document") => ViewType::Document,
            //     Some("numbered") => ViewType::Numbered,
            //     _ => ViewType::Bullet,
            // };

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
                // TODO Get this from attrs
                view_type,
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

    fn add_block_and_children(
        &mut self,
        parent_id: Option<usize>,
        page_id: Option<usize>,
        json_block: &JsonBlock,
    ) -> Result<usize, anyhow::Error> {
        let tags = json_block
            .properties
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|array| array.iter().map(json_to_string).collect::<SmallVec<_>>())
            .unwrap_or_default();

        let attrs = json_block
            .properties
            .iter()
            .map(|(k, v)| {
                // Convert an array to an array of each string, or a singleton to a one-value array.
                let values = v
                    .as_array()
                    .map(|values| values.iter().map(json_to_string).collect::<SmallVec<_>>())
                    .unwrap_or_else(|| smallvec![json_to_string(v)]);

                (k.clone(), values)
            })
            .collect::<FxHashMap<_, _>>();

        let this_id = self.next_id;
        self.next_id += 1;

        let page_id = page_id.or(Some(this_id));

        let mut children = SmallVec::new();
        for child in &json_block.children {
            let child_id = self.add_block_and_children(Some(this_id), page_id, child)?;
            children.push(child_id);
        }

        let heading = json_block.heading_level.unwrap_or_else(|| {
            match json_block
                .properties
                .get("heading")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                true => 1,
                false => 0,
            }
        });

        let block = Block {
            id: this_id,
            uid: json_block.id.clone(),
            page_title: json_block.page_name.clone(),
            containing_page: page_id.unwrap(),
            attrs,
            // format: json_block.format.unwrap_or(BlockFormat::Unknown),
            string: json_block.content.clone().unwrap_or_default(),
            heading,

            parent: parent_id,
            children,
            tags,

            order: 0,
            view_type: crate::graph::ViewType::Bullet,
            is_journal: false,
            create_time: 0,
            edit_time: 0,
        };

        self.next_id += 1;

        self.graph.add_block(block);

        Ok(this_id)
    }
}

struct LogseqRawPage {
    base_id: usize,
    attrs: FxHashMap<String, AttrList>,
    blocks: Vec<LogseqRawBlock>,
}

fn read_logseq_md_file(filename: &Path) -> Result<LogseqRawPage, anyhow::Error> {
    let file =
        File::open(filename).with_context(|| format!("Reading {}", filename.to_string_lossy()))?;
    let mut lines = put_back(BufReader::new(file).lines());

    let mut page_attrs = page_header::parse_page_header(&mut lines)?;
    let blocks = blocks::parse_raw_blocks(&mut lines)?;

    if !page_attrs.contains_key("title") {
        let title = filename
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .expect("file title");
        page_attrs.insert(String::from("title"), smallvec![title]);
    }

    Ok(LogseqRawPage {
        base_id: 0,
        attrs: page_attrs,
        blocks,
    })
}

/// Convert a JSON value to a string, without the quotes around strings
fn json_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.to_string(),
        _ => value.to_string(),
    }
}
