use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{anyhow, Context};
use edn_rs::Edn;
use fxhash::FxHashMap;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::{complete::multispace0, is_space},
    combinator::{all_consuming, map, opt},
    multi::{many0, separated_list0},
    sequence::{delimited, preceded},
    IResult,
};
use rayon::prelude::*;
use serde::Deserialize;
use smallvec::{smallvec, SmallVec};

use crate::{
    graph::{Block, Graph},
    parse_string::{hashtag, link_or_word},
};

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

pub struct PageMetadata {
    created_time: usize,
    edited_time: usize,
}

pub struct LogseqGraph {
    next_id: usize,
    root: PathBuf,
    graph: Graph,

    page_metadata: FxHashMap<String, PageMetadata>,
}

impl LogseqGraph {
    // This is a weird way to do it since the "constructor" returns a Graph instead of a
    // LogseqGraph, but there's no reason to do otherwise in this case since we never actually want
    // to keep the LogseqGraph around.
    pub fn build(path: PathBuf) -> Result<Graph, anyhow::Error> {
        let mut lsgraph = LogseqGraph {
            next_id: 0,
            graph: Graph::new(crate::parse_string::ContentStyle::Logseq, false),
            root: path,
            page_metadata: FxHashMap::default(),
        };

        lsgraph.read_page_metadata()?;
        lsgraph.read_page_directory("pages", false)?;
        lsgraph.read_page_directory("journals", true)?;

        // for block in file.blocks {
        //     lsgraph.add_block_and_children(None, None, &block)?;
        // }

        Ok(lsgraph.graph)
    }

    fn read_page_metadata(&mut self) -> Result<(), anyhow::Error> {
        let metadata_path = self.root.join("logseq").join("pages-metadata.edn");
        let source = std::fs::read_to_string(metadata_path)?;
        let data = Edn::from_str(source.as_str())?;

        let blocks = match data {
            Edn::Vector(blocks) => blocks.to_vec(),
            _ => return Err(anyhow!("Unknown page-metadata format, expected list")),
        };

        self.page_metadata = blocks
            .into_iter()
            .map(|data| {
                let block_name = data
                    .get(":block/name")
                    .map(|v| v.to_string())
                    .ok_or_else(|| anyhow!("No block name found in page-metadata block"))?;
                let created_time = data
                    .get(":block/created-at")
                    .and_then(|v| v.to_uint())
                    .unwrap_or(0);
                let edited_time = data
                    .get(":block/edited-at")
                    .and_then(|v| v.to_uint())
                    .unwrap_or(0);

                Ok((
                    block_name,
                    PageMetadata {
                        created_time,
                        edited_time,
                    },
                ))
            })
            .collect::<Result<_, anyhow::Error>>()?;

        Ok(())
    }

    fn read_page_directory(&mut self, name: &str, is_journal: bool) -> Result<(), anyhow::Error> {
        let dir = self.root.join(name);
        let files = std::fs::read_dir(&dir)?
            .map(|f| f.map(|f| f.path()))
            .collect::<Result<Vec<_>, _>>()?;

        let blocks = files
            .par_iter()
            .map(|file| read_logseq_md_file(file))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(())
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HeaderParseState {
    None,
    YamlFrontMatter,
    AttrFrontMatter,
}

fn read_logseq_md_file(filename: &Path) -> Result<Vec<Block>, anyhow::Error> {
    let file =
        File::open(filename).with_context(|| format!("Reading {}", filename.to_string_lossy()))?;
    let mut lines = BufReader::new(file).lines();

    let (first_line, page_attrs) = parse_page_header(&mut lines)?;

    let blocks = Vec::new();

    Ok(blocks)
}

fn parse_page_header(
    lines: &mut std::io::Lines<impl BufRead>,
) -> Result<(String, FxHashMap<String, Vec<String>>), anyhow::Error> {
    let mut page_attrs = FxHashMap::default();
    let first_line = lines.next().transpose()?.unwrap_or_default();
    if first_line.is_empty() {
        return Ok((first_line, page_attrs));
    }

    let parse_attr_line = |separator, line: &str| {
        let parsed: Option<Result<_, anyhow::Error>> =
            line.split_once(separator)
                .map(|(attr_name, attr_value_str)| {
                    let attr_value_str = attr_value_str.trim();
                    let values = if attr_name == "tags" {
                        parse_tag_values(attr_value_str)?
                    } else {
                        vec![attr_value_str.to_string()]
                    };

                    Ok((attr_name.to_string(), values))
                });

        parsed
    };

    let header_state: HeaderParseState;
    if first_line.trim_end() == "---" {
        header_state = HeaderParseState::YamlFrontMatter;
    } else if !first_line.starts_with('-') {
        // Logseq Attribute front matter style

        // The first line is actually an attribute so we need to parse it.
        let parsed = parse_attr_line("::", first_line.as_str());

        match parsed {
            Some(Ok((attr_name, attr_values))) => {
                header_state = HeaderParseState::AttrFrontMatter;
                page_attrs.insert(attr_name, attr_values);
            }
            _ => {
                // It wasn't actually an attribute, so exit header parse mode.
                header_state = HeaderParseState::None;
            }
        };
    } else {
        header_state = HeaderParseState::None;
    }

    let line = if header_state == HeaderParseState::None {
        first_line
    } else {
        loop {
            let line = match (header_state, lines.next()) {
                (_, None) => {
                    return Ok((String::new(), page_attrs));
                }
                (_, Some(Err(e))) => return Err(e.into()),
                (HeaderParseState::None, _) => panic!("In header parse where state is None"),
                (HeaderParseState::AttrFrontMatter, Some(Ok(line))) => {
                    if line.starts_with('-') {
                        // This is the start of the real content, so return the line.
                        break line;
                    }
                    line
                }
                (HeaderParseState::YamlFrontMatter, Some(Ok(line))) => {
                    if line == "---" {
                        // This is the end of the header, but not real content, so just return an
                        // empty string.
                        break String::new();
                    }
                    line
                }
            };

            let separator = if header_state == HeaderParseState::YamlFrontMatter {
                ":"
            } else {
                "::"
            };

            let parsed = parse_attr_line(separator, line.as_str());

            match parsed {
                Some(Ok((attr_name, attr_values))) => page_attrs.insert(attr_name, attr_values),
                _ => break line,
            };
        }
    };

    Ok((line, page_attrs))
}

fn tag_value_separator(input: &str) -> IResult<&str, &str> {
    take_while1(|c| is_space(c as u8) || c == ',')(input)
}

fn parse_tag_value(input: &str) -> IResult<&str, &str> {
    alt((map(hashtag, |(value, _)| value), link_or_word))(input)
}

fn parse_tag_values(input: &str) -> Result<Vec<String>, anyhow::Error> {
    let values = match separated_list0(tag_value_separator, parse_tag_value)(input) {
        Ok((_, values)) => values,
        Err(e) => return Err(anyhow!("Parsing {}: {}", input, e)),
    };

    Ok(values.iter().map(|v| v.to_string()).collect::<Vec<_>>())
}

/// Convert a JSON value to a string, without the quotes around strings
fn json_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod test {

    mod page_header {
        use std::{io::BufRead, iter::FromIterator};

        use fxhash::FxHashMap;
        use indoc::indoc;

        use super::super::parse_page_header;

        fn run_test(
            input: &str,
        ) -> Result<(String, FxHashMap<String, Vec<String>>), anyhow::Error> {
            let reader = std::io::BufReader::new(input.as_bytes());
            parse_page_header(&mut reader.lines())
        }

        #[test]
        fn no_frontmatter() {
            let input = r##"- the first block
                - another block
                "##;

            assert_eq!(
                run_test(input).unwrap(),
                (
                    String::from("- the first block"),
                    FxHashMap::<String, Vec<String>>::default()
                )
            );
        }

        #[test]
        fn empty_yaml_frontmatter() {
            let input = indoc! { r##"
                ---
                ---
                - the first block
                - another block
                "##
            };

            println!("{}", input);

            assert_eq!(
                run_test(input).unwrap(),
                (String::new(), FxHashMap::<String, Vec<String>>::default())
            );
        }

        #[test]
        fn yaml_frontmatter() {
            let input = indoc! { r##"
                ---
                title: It's a title
                tags: a, b, c
                ---
                - some text
                "##


            };

            assert_eq!(
                run_test(input).unwrap(),
                (
                    String::new(),
                    FxHashMap::<String, Vec<String>>::from_iter([
                        (String::from("title"), vec![String::from("It's a title")]),
                        (
                            String::from("tags"),
                            vec![String::from("a"), String::from("b"), String::from("c")]
                        )
                    ])
                )
            );
        }

        #[test]
        fn attr_frontmatter() {
            let input = indoc! { r##"
                title:: It's a title
                tags:: a, b, c
                - some text
                "##


            };

            assert_eq!(
                run_test(input).unwrap(),
                (
                    String::from("- some text"),
                    FxHashMap::<String, Vec<String>>::from_iter([
                        (String::from("title"), vec![String::from("It's a title")]),
                        (
                            String::from("tags"),
                            vec![String::from("a"), String::from("b"), String::from("c")]
                        )
                    ])
                )
            );
        }
    }

    mod tag_values {
        use super::super::{parse_tag_value, parse_tag_values, tag_value_separator};

        #[test]
        fn separator() {
            tag_value_separator(" ").expect("parsing space");
            tag_value_separator(",").expect("parsing comma");
            tag_value_separator(", ").expect("parsing comma with trailing space");
            tag_value_separator(" ,").expect("parsing comma with leading space");
            tag_value_separator(" , ").expect("parsing comman with spaces on both sides");
        }

        #[test]
        fn single_tag_values() {
            assert_eq!(parse_tag_value("#abc").expect("hashtag"), ("", "abc"));
            assert_eq!(parse_tag_value("abc").expect("raw value"), ("", "abc"));
            assert_eq!(
                parse_tag_value("[[abc def]]").expect("link"),
                ("", "abc def")
            );
        }

        #[test]
        fn one_hashtag() {
            assert_eq!(parse_tag_values("#abc").expect("parsing"), vec!["abc"])
        }

        #[test]
        fn two_hashtags() {
            assert_eq!(
                parse_tag_values("#abc #def").expect("parsing"),
                vec!["abc", "def"]
            )
        }

        #[test]
        fn two_raw_values() {
            assert_eq!(
                parse_tag_values("abc def").expect("parsing"),
                vec!["abc", "def"]
            )
        }

        #[test]
        fn hashtags_with_commas() {
            assert_eq!(
                parse_tag_values("#abc, #def").expect("parsing"),
                vec!["abc", "def"]
            )
        }

        #[test]
        fn values_with_commas() {
            assert_eq!(
                parse_tag_values("abc, def").expect("parsing"),
                vec!["abc", "def"]
            )
        }
    }
}
