use std::collections::BTreeMap;

use fxhash::FxHashMap;
use smallvec::SmallVec;

pub struct Block {
    pub id: usize,
    pub containing_page: usize,
    pub page_title: Option<String>,
    pub uid: String,

    pub tags: SmallVec<[usize; 2]>,
}

pub struct Graph {
    pub blocks: BTreeMap<usize, Block>,
    pub titles: FxHashMap<String, usize>,
    pub blocks_by_uid: FxHashMap<String, usize>,

    pub tags: Vec<String>,
    pub tags_by_name: FxHashMap<String, usize>,
}

impl Graph {
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
        references: &'a [usize],
    ) -> impl Iterator<Item = &'a Block> {
        self.block_iter(move |(_, n)| n.tags.iter().any(move |r| references.contains(r)))
    }

    pub fn block_from_uid(&self, uid: &str) -> Option<&Block> {
        self.blocks_by_uid
            .get(uid)
            .and_then(|id| self.blocks.get(id))
    }
}
