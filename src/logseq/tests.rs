use std::io::BufRead;

use fxhash::FxHashMap;
use itertools::{put_back, Itertools};
use smallvec::smallvec;

use crate::graph::ViewType;
use crate::logseq::LogseqRawPage;

use crate::logseq::blocks::LogseqRawBlock;

#[test]
fn full_page() {
    let source = r##"title:: Circa
tags:: Project

- # Some tools
- Based on
	- a book
	- another book
- ## Data Model Graph
	- A mostly-DAG
	- because of some exceptions
	  view-mode:: numbered
		- Exception 1
			- maybe not
		- Exception 2
		  id:: b4eb8b3b-9d09-4358-8e05-0d29e4301ecb
- Closing notes
"##;

    let mut reader = put_back(std::io::BufReader::new(source.as_bytes()).lines());
    let filename = std::path::PathBuf::from("abc/the filename.md");
    let parsed = super::parse_logseq_file(&filename, &mut reader).expect("parsing");

    let expected_blocks = vec![
        LogseqRawBlock {
            header_level: 1,
            contents: String::from("Some tools"),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("Based on"),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("a book"),
            indent: 1,
            parent_idx: Some(1),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("another book"),
            indent: 1,
            parent_idx: Some(1),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("Data Model Graph"),
            header_level: 2,
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("A mostly-DAG"),
            indent: 1,
            parent_idx: Some(4),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("because of some exceptions"),
            indent: 1,
            parent_idx: Some(4),
            view_type: ViewType::Numbered,
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("Exception 1"),
            indent: 2,
            parent_idx: Some(6),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("maybe not"),
            indent: 3,
            parent_idx: Some(7),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            id: String::from("b4eb8b3b-9d09-4358-8e05-0d29e4301ecb"),
            contents: String::from("Exception 2"),
            indent: 2,
            parent_idx: Some(6),
            ..LogseqRawBlock::default()
        },
        LogseqRawBlock {
            contents: String::from("Closing notes"),
            ..LogseqRawBlock::default()
        },
    ];

    let expected_attrs = FxHashMap::from_iter([
        (String::from("title"), smallvec![String::from("Circa")]),
        (String::from("tags"), smallvec![String::from("Project")]),
    ]);

    assert_eq!(parsed.attrs, expected_attrs, "page attributes");

    for (i, items) in parsed
        .blocks
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
