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

pub struct LogseqGraph {
    next_id: usize,

    graph: Graph,
}

impl LogseqGraph {
    // This is a weird way to do it since the "constructor" returns a Graph instead of a
    // LogseqGraph, but there's no reason to do otherwise in this case since we never actually want
    // to keep the LogseqGraph around.
    pub fn from_json(data: &str) -> Result<Graph, anyhow::Error> {
        let file: JsonFile = serde_json::from_str(data)?;

        let mut lsgraph = LogseqGraph {
            next_id: 0,
            graph: Graph::new(crate::parse_string::ContentStyle::Logseq, false),
        };

        for block in file.blocks {
            lsgraph.add_block_and_children(None, None, &block)?;
        }

        Ok(lsgraph.graph)
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

pub fn graph_from_logseq_json(data: &str) -> Result<Graph, anyhow::Error> {
    LogseqGraph::from_json(data)
}

/// Convert a JSON value to a string, without the quotes around strings
fn json_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.to_string(),
        _ => value.to_string(),
    }
}
