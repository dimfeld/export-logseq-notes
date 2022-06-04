use std::{collections::BTreeMap, fs::File, io::BufReader};

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
    Unknown,
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
    #[serde(rename = "heading-level")]
    pub heading_level: Option<usize>,
}

#[derive(Debug)]
pub struct Block {
    pub id: String,
    pub page_name: Option<String>,
    pub properties: FxHashMap<String, serde_json::Value>,
    pub format: BlockFormat,
    pub content: Option<String>,

    // Values derived from the JSON block format
    pub public: bool,
    pub tags: Vec<String>,
    pub children: SmallVec<[String; 2]>,
    pub heading: usize,
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

        let file = BufReader::new(File::open(filename)?);
        let file: JsonFile = serde_json::from_reader(file)?;

        for block in file.blocks {
            graph.add_block_and_children(block)?;
        }

        Ok(graph)
    }

    fn add_block_and_children(&mut self, json_block: JsonBlock) -> Result<(), anyhow::Error> {
        let public = json_block
            .properties
            .get("public")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let tags = json_block
            .properties
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|array| array.iter().map(|v| v.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();

        let mut children = SmallVec::new();
        for child in json_block.children {
            children.push(child.id.clone());
            self.add_block_and_children(child)?;
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
            id: json_block.id,
            page_name: json_block.page_name,
            properties: json_block.properties,
            format: json_block.format.unwrap_or(BlockFormat::Unknown),
            content: json_block.content,
            heading,

            children,
            public,
            tags,
        };

        if let Some(title) = block.page_name.as_ref() {
            self.titles.insert(title.to_string(), block.id.clone());
        }
        self.blocks.insert(block.id.clone(), block);

        Ok(())
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
