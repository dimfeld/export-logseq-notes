use std::collections::BTreeMap;

use fxhash::FxHashMap;
use serde::Deserialize;
use smallvec::{smallvec, SmallVec};

use crate::graph::{Block, Graph};

#[derive(Deserialize, Debug)]
pub struct JsonFile {
    version: usize,
    blocks: Vec<JsonBlock>,
}

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

#[derive(Debug)]
pub struct LogseqBlock {
    pub id: usize,
    /// The ID of the block from Logseq
    pub uid: String,
    pub page_name: Option<String>,
    pub containing_page: usize,
    pub properties: FxHashMap<String, serde_json::Value>,
    pub format: BlockFormat,
    pub content: Option<String>,

    // Values derived from the JSON block format
    pub public: bool,
    pub tags: SmallVec<[String; 2]>,
    pub attrs: FxHashMap<String, SmallVec<[String; 1]>>,
    pub children: SmallVec<[usize; 2]>,
    pub parent: Option<usize>,
    pub heading: usize,
}

pub struct LogseqGraph {
    pub blocks: BTreeMap<usize, LogseqBlock>,
    // Map of titles to page IDs
    pub titles: FxHashMap<String, usize>,

    next_id: usize,
}

impl LogseqGraph {
    pub fn from_json(data: &str) -> Result<LogseqGraph, anyhow::Error> {
        let mut graph = LogseqGraph {
            blocks: BTreeMap::new(),
            titles: FxHashMap::default(),
            next_id: 0,
        };

        let file: JsonFile = serde_json::from_str(data)?;

        for block in file.blocks {
            graph.add_block_and_children(None, None, &block)?;
        }

        Ok(graph)
    }

    fn add_block_and_children(
        &mut self,
        parent_id: Option<usize>,
        page_id: Option<usize>,
        json_block: &JsonBlock,
    ) -> Result<usize, anyhow::Error> {
        let public = json_block
            .properties
            .get("public")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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

        let block = LogseqBlock {
            id: this_id,
            uid: json_block.id.clone(),
            page_name: json_block.page_name.clone(),
            containing_page: page_id.unwrap(),
            properties: json_block.properties.clone(),
            format: json_block.format.unwrap_or(BlockFormat::Unknown),
            content: json_block.content.clone(),
            heading,

            parent: parent_id,
            children,
            public,
            tags,
            attrs,
        };

        self.next_id += 1;

        if let Some(title) = block.page_name.as_ref() {
            self.titles.insert(title.to_string(), block.id);
        }
        self.blocks.insert(this_id, block);

        Ok(this_id)
    }
}

pub fn graph_from_logseq_json(path: &str) -> Result<Graph, anyhow::Error> {
    let mut logseq_graph = LogseqGraph::from_json(path)?;
    let mut graph = Graph::new(crate::parse_string::ContentStyle::Logseq, false);

    for lsblock in logseq_graph.blocks.values_mut() {
        let block = Block {
            id: lsblock.id,
            uid: lsblock.uid.clone(),
            tags: lsblock.tags.clone(),
            attrs: lsblock.attrs.clone(),
            heading: lsblock.heading,
            page_title: lsblock.page_name.clone(),
            order: 0,
            create_time: 0,
            string: lsblock.content.take().unwrap_or_default(),
            view_type: crate::graph::ViewType::Bullet,

            parent: lsblock.parent,
            children: lsblock.children.clone(),
            containing_page: lsblock.containing_page,

            // TODO Figure out a good way to get these values.
            is_journal: false,
            edit_time: 0,
        };

        graph.add_block(block);
    }

    Ok(graph)
}

/// Convert a JSON value to a string, without the quotes around strings
fn json_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.to_string(),
        _ => value.to_string(),
    }
}
