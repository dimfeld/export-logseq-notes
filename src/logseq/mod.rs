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
use rayon::prelude::*;
use serde::Deserialize;
use smallvec::{smallvec, SmallVec};

use crate::graph::{Block, Graph};

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

        let blocks = files
            .par_iter()
            .map(|file| read_logseq_md_file(file))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(())
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

fn read_logseq_md_file(filename: &Path) -> Result<Vec<Block>, anyhow::Error> {
    let file =
        File::open(filename).with_context(|| format!("Reading {}", filename.to_string_lossy()))?;
    let mut lines = BufReader::new(file).lines();

    let (first_line, page_attrs) = page_header::parse_page_header(&mut lines)?;

    let blocks = blocks::parse_blocks(first_line, &mut lines)?;

    Ok(blocks)
}

/// Convert a JSON value to a string, without the quotes around strings
fn json_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.to_string(),
        _ => value.to_string(),
    }
}
