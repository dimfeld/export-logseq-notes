use std::collections::BTreeMap;

use fxhash::FxHashMap;
use serde::Deserialize;
use smallvec::SmallVec;

#[derive(Deserialize, Debug)]
pub struct JsonFile {
    version: usize,
    blocks: Vec<JsonBlock>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum BlockFormat {
    Markdown,
}

#[derive(Deserialize, Debug)]
pub struct JsonBlock {
    pub id: String,
    #[serde(rename = "page-name")]
    pub page_name: Option<String>,
    pub properties: FxHashMap<String, serde_json::Value>,
    pub children: Vec<JsonBlock>,
    pub format: Option<BlockFormat>,
    pub content: Option<String>,
}

#[derive(Debug)]
pub struct Block {
    pub id: String,
    pub page_name: Option<String>,
    pub properties: FxHashMap<String, serde_json::Value>,
    pub format: Option<BlockFormat>,
    pub content: Option<String>,

    // Values derived from the JSON block format
    pub public: bool,
    pub tags: Vec<String>,
    pub children: SmallVec<[String; 2]>,
}

pub struct Graph {
    pub blocks: BTreeMap<String, Block>,
    // Map of titles to page IDs
    pub titles: FxHashMap<String, String>,
}

impl Graph {
    pub fn from_json(filename: &str) -> Result<Graph, anyhow::Error> {
        let mut graph = Graph {
            blocks: BTreeMap::new(),
            titles: FxHashMap::default(),
        };

        Ok(graph)
    }

    fn block_iter<F: FnMut(&(&String, &Block)) -> bool>(
        &self,
        filter: F,
    ) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(filter).map(|(_, n)| n)
    }

    pub fn pages(&self) -> impl Iterator<Item = &Block> {
        self.block_iter(|(_, n)| n.page_name.is_some())
    }
}
