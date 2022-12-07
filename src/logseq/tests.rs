use std::io::BufRead;

use ahash::HashMap;
use itertools::{put_back, Itertools};
use smallvec::smallvec;

use crate::{
    content::BlockContent,
    graph::ViewType,
    logseq::{blocks::LogseqRawBlock, LogseqRawPage},
    parse_string::ContentStyle,
};

fn new_content(s: impl Into<String>) -> BlockContent {
    BlockContent::new_parsed(ContentStyle::Logseq, s.into()).unwrap()
}

#[test]
fn full_page() {
    let source = r##"title:: Circa
Tags:: Project

- # Some tools
- Based on
  view-mode:: document
	- a book
	- another book
- ## Data Model Graph
	- A mostly-DAG
	- because of some exceptions
	  id:: 93804e07-d826-44bc-94f4-18b07b0052b6
	  view-mode:: numbered
		- Exception 1
			- maybe not
		- Exception 2
		  id:: b4eb8b3b-9d09-4358-8e05-0d29e4301ecb
- Closing notes
"##;

    let mut reader = put_back(std::io::BufReader::new(source.as_bytes()).lines());
    let filename = std::path::PathBuf::from("abc/the filename.md");
    let parsed = super::parse_logseq_file(&filename, &mut reader, false).expect("parsing");

    let expected_blocks = vec![
        LogseqRawBlock {
            contents: new_content("Tags:: Project"),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            header_level: 1,
            contents: new_content("Some tools"),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("Based on"),
            view_type: ViewType::Document,
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("a book"),
            indent: 1,
            parent_idx: Some(2),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("another book"),
            indent: 1,
            parent_idx: Some(2),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("Data Model Graph"),
            header_level: 2,
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("A mostly-DAG"),
            indent: 1,
            parent_idx: Some(5),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("because of some exceptions"),
            id: String::from("93804e07-d826-44bc-94f4-18b07b0052b6"),
            indent: 1,
            parent_idx: Some(5),
            view_type: ViewType::Numbered,
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("Exception 1"),
            indent: 2,
            parent_idx: Some(7),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("maybe not"),
            indent: 3,
            parent_idx: Some(8),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            id: String::from("b4eb8b3b-9d09-4358-8e05-0d29e4301ecb"),
            contents: new_content("Exception 2"),
            indent: 2,
            parent_idx: Some(7),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: new_content("Closing notes"),
            ..LogseqRawBlock::default()
        },
    ];

    let expected_attrs = HashMap::from_iter([
        (String::from("title"), smallvec![String::from("Circa")]),
        (String::from("tags"), smallvec![String::from("Project")]),
    ]);

    assert_eq!(parsed.0, expected_attrs, "page attributes");

    for (i, items) in parsed
        .1
        .iter()
        .zip_longest(expected_blocks.iter())
        .enumerate()
    {
        match items {
            itertools::EitherOrBoth::Both(parsed, expected) => {
                assert_eq!(parsed, expected, "item {}", i);
                println!("{parsed:?} success");
            }
            itertools::EitherOrBoth::Left(parsed) => {
                panic!("Extra element {parsed:?}");
            }
            itertools::EitherOrBoth::Right(expected) => {
                panic!("Expected to see element {expected:?}");
            }
        }
    }
}
