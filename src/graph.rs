use std::collections::BTreeMap;

use fxhash::FxHashMap;
use smallvec::SmallVec;

use crate::parse_string::ContentStyle;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ViewType {
    Bullet,
    Numbered,
    Document,
}

impl Default for ViewType {
    fn default() -> ViewType {
        ViewType::Bullet
    }
}

impl<T> From<T> for ViewType
where
    for<'a> T: AsRef<str>,
{
    fn from(value: T) -> Self {
        match value.as_ref() {
            "document" => ViewType::Document,
            "numbered" => ViewType::Numbered,
            _ => ViewType::Bullet,
        }
    }
}

pub type AttrList = SmallVec<[String; 1]>;

#[derive(Debug)]
pub struct Block {
    pub id: usize,
    pub containing_page: usize,
    pub page_title: Option<String>,
    pub uid: String,

    pub parent: Option<usize>,
    pub children: SmallVec<[usize; 2]>,
    pub order: usize,

    pub tags: AttrList,
    pub attrs: FxHashMap<String, AttrList>,
    pub is_journal: bool,

    pub string: String,
    pub heading: usize,
    pub view_type: ViewType,

    pub edit_time: usize,
    pub create_time: usize,
}

pub struct Graph {
    pub blocks: BTreeMap<usize, Block>,
    pub titles: FxHashMap<String, usize>,
    pub blocks_by_uid: FxHashMap<String, usize>,

    /// true if the blocks are ordered by the order field, instead of just the order in which they
    /// appear in `children`
    pub block_explicit_ordering: bool,

    pub content_style: ContentStyle,
    pub tagged_blocks: FxHashMap<String, Vec<usize>>,
}

impl Graph {
    pub fn new(content_style: ContentStyle, block_explicit_ordering: bool) -> Graph {
        Graph {
            blocks: BTreeMap::new(),
            titles: FxHashMap::default(),
            blocks_by_uid: FxHashMap::default(),
            tagged_blocks: FxHashMap::default(),
            content_style,
            block_explicit_ordering,
        }
    }

    pub fn add_block(&mut self, block: Block) {
        if let Some(title) = block.page_title.as_ref() {
            self.titles.insert(title.clone(), block.id);
        }

        for tag in block.tags.iter() {
            self.tagged_blocks
                .entry(tag.clone())
                .or_default()
                .push(block.id);
        }

        self.blocks_by_uid.insert(block.uid.clone(), block.id);
        self.blocks.insert(block.id, block);
    }

    fn block_iter<F: FnMut(&(&usize, &Block)) -> bool>(
        &self,
        filter: F,
    ) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(filter).map(|(_, n)| n)
    }

    pub fn pages(&self) -> impl Iterator<Item = &Block> {
        self.block_iter(|(_, n)| n.page_title.is_some())
    }

    pub fn blocks_with_references<'a>(
        &'a self,
        references: &'a [&'a str],
    ) -> impl Iterator<Item = &'a Block> {
        self.block_iter(move |(_, n)| {
            n.tags
                .iter()
                .any(move |tag| references.iter().any(|r| tag == r))
        })
    }

    pub fn block_from_uid(&self, uid: &str) -> Option<&Block> {
        self.blocks_by_uid
            .get(uid)
            .and_then(|id| self.blocks.get(id))
    }
}
