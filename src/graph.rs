use ahash::HashMap;
use smallvec::SmallVec;

use crate::parse_string::ContentStyle;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum ViewType {
    #[default]
    Inherit,
    Bullet,
    Numbered,
    Document,
}

impl<T> From<T> for ViewType
where
    for<'a> T: AsRef<str>,
{
    fn from(value: T) -> Self {
        match value.as_ref() {
            "document" => ViewType::Document,
            "numbered" => ViewType::Numbered,
            "bullet" => ViewType::Bullet,
            _ => ViewType::Inherit,
        }
    }
}

impl ViewType {
    pub fn default_view_type() -> ViewType {
        ViewType::Bullet
    }

    pub fn resolve_with_parent(&self, parent: ViewType) -> ViewType {
        match self {
            ViewType::Inherit => parent,
            _ => *self,
        }
    }
}

pub type AttrList = SmallVec<[String; 1]>;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum BlockInclude {
    /// Render the block and its children.
    #[default]
    AndChildren,
    /// Skip rendering the block, but render its children.
    OnlyChildren,
    /// Render just this block and not its children.
    JustBlock,
    /// Don't render the block or its children.
    Exclude,
    /// Render the block and its children, if the children have content
    IfChildrenPresent,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: usize,
    pub containing_page: usize,
    pub page_title: Option<String>,
    pub uid: String,

    pub parent: Option<usize>,
    pub children: SmallVec<[usize; 2]>,
    pub order: usize,
    pub include_type: BlockInclude,

    pub tags: AttrList,
    pub attrs: HashMap<String, AttrList>,
    pub is_journal: bool,

    pub string: String,
    pub heading: usize,
    pub view_type: ViewType,

    pub edit_time: usize,
    pub create_time: usize,
}

#[derive(Debug)]
pub struct Graph {
    pub blocks: HashMap<usize, Block>,
    pub blocks_by_uid: HashMap<String, usize>,
    pub page_blocks: Vec<usize>,

    /// true if the blocks are ordered by the order field, instead of just the order in which they
    /// appear in `children`
    pub block_explicit_ordering: bool,

    pub content_style: ContentStyle,
    pub tagged_blocks: HashMap<String, Vec<usize>>,
}

impl Graph {
    pub fn new(content_style: ContentStyle, block_explicit_ordering: bool) -> Graph {
        Graph {
            blocks: HashMap::default(),
            blocks_by_uid: HashMap::default(),
            page_blocks: Vec::new(),
            tagged_blocks: HashMap::default(),
            content_style,
            block_explicit_ordering,
        }
    }

    pub fn add_block(&mut self, block: Block) {
        if block.page_title.is_some() {
            self.page_blocks.push(block.id);
        }

        for tag in block.tags.iter() {
            self.tagged_blocks
                .entry(tag.clone())
                .or_default()
                .push(block.id);
        }

        if !block.uid.is_empty() {
            self.blocks_by_uid.insert(block.uid.clone(), block.id);
        }
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
