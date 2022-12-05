use eyre::eyre;
use ouroboros::self_referencing;

use crate::parse_string::{parse, ContentStyle, Expression};

#[self_referencing]
#[derive(Debug, Eq)]
pub struct BlockContent {
    pub style: ContentStyle,
    pub string: String,
    #[borrows(string)]
    #[covariant]
    pub parsed: Vec<Expression<'this>>,
}

impl Clone for BlockContent {
    fn clone(&self) -> Self {
        let string = self.borrow_string().clone();
        let style = self.borrow_style().clone();

        BlockContentBuilder {
            style,
            string,
            parsed_builder: |s| parse(style, s).unwrap(),
        }
        .build()
    }
}

impl PartialEq for BlockContent {
    fn eq(&self, other: &Self) -> bool {
        self.borrow_string() == other.borrow_string()
    }
}

impl Default for BlockContent {
    fn default() -> Self {
        // Content style doesn't really matter for an empty block so just choose one.
        Self::new_empty(ContentStyle::Logseq)
    }
}

impl BlockContent {
    pub fn new_parsed(style: ContentStyle, content: String) -> eyre::Result<BlockContent> {
        BlockContentTryBuilder {
            style,
            string: content,
            parsed_builder: |s| parse(style, s.as_str()).map_err(|e| eyre!("{:?}", e)),
        }
        .try_build()
        .map_err(eyre::Error::from)
    }

    pub fn new_empty(style: ContentStyle) -> BlockContent {
        BlockContentBuilder {
            style,
            string: String::new(),
            parsed_builder: |_| Vec::new(),
        }
        .build()
    }
}
