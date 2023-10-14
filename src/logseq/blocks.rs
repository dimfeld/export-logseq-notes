use std::io::BufRead;

use ahash::HashMap;
use eyre::{eyre, Result};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::multispace0,
    combinator::{all_consuming, map, opt},
    sequence::{preceded, terminated, tuple},
    IResult,
};
use smallvec::SmallVec;

use super::{attrs::parse_attr_line, LinesIterator};
use crate::{
    content::BlockContent,
    graph::{AttrList, ListType, ViewType},
    parse_string::{self, Expression},
};

#[derive(Debug, PartialEq, Eq)]
struct Line<'a> {
    contents: &'a str,
    indent: u32,
    header: u32,
    new_block: bool,
    attr_name: String,
    attr_values: AttrList,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LogseqRawBlock {
    pub id: String,
    pub parent_idx: Option<usize>,
    pub header_level: u32,
    pub contents: BlockContent,
    pub view_type: ViewType,
    pub this_block_list_type: ListType,
    pub collapsed: bool,
    pub indent: u32,
    pub tags: AttrList,
    pub attrs: HashMap<String, AttrList>,
}

pub fn parse_raw_blocks(
    blocks: &mut Vec<LogseqRawBlock>,
    lines: &mut LinesIterator<impl BufRead>,
) -> Result<()> {
    let mut current_indent = 0;
    let mut current_parent: Option<usize> = None;
    loop {
        match read_raw_block(lines)? {
            RawBlockOutput::Done => break,
            RawBlockOutput::Empty => {}
            RawBlockOutput::Block(mut block) => {
                if block.indent > current_indent {
                    // A new child of the previous block.
                    current_parent = Some(blocks.len() - 1);
                } else if block.indent < current_indent {
                    // Going up a level. Find the most recent block in the list
                    // with the same indent level, and use its parent.
                    current_parent = blocks
                        .iter()
                        .rfind(|b| b.indent == block.indent)
                        .and_then(|block| block.parent_idx);
                }
                // otherwise it's a sibling so it has the same parent

                current_indent = block.indent;
                block.parent_idx = current_parent;

                blocks.push(block);
            }
        }
    }

    Ok(())
}

enum RawBlockOutput {
    Done,
    Empty,
    Block(LogseqRawBlock),
}

fn read_raw_block(lines: &mut LinesIterator<impl BufRead>) -> Result<RawBlockOutput> {
    // Most blocks will just be one or two lines
    let mut line_contents: SmallVec<[String; 2]> = SmallVec::new();
    let mut indent = 0;
    let mut id = String::new();
    let mut view_type = ViewType::Inherit;
    let mut this_block_list_type = ListType::Default;
    let mut header = 0;
    let mut collapsed = false;
    let mut attrs = HashMap::default();

    let mut all_done = false;
    let mut in_code_block = false;
    let mut in_logbook = false;

    loop {
        let line_read = lines.next();
        if let Some(line) = line_read {
            let line = line?;
            if line.is_empty() && !in_code_block {
                continue;
            }

            let parsed = evaluate_line(line.as_str(), in_code_block)?;
            match parsed {
                None => break,
                Some(mut parsed) => {
                    // YAML inside code blocks can throw off the parser. This hacks around
                    // that.
                    let has_triple = parsed.contents.contains("```");
                    let enter_code_block = has_triple && !in_code_block;
                    let leave_code_block = has_triple && in_code_block;
                    if leave_code_block {
                        in_code_block = false;
                    }

                    if parsed.contents == ":LOGBOOK:" {
                        in_logbook = true;
                        continue;
                    } else if in_logbook {
                        if parsed.contents == ":END" || parsed.new_block {
                            in_logbook = false;
                        } else {
                            continue;
                        }
                    }

                    if line_contents.is_empty() {
                        // Force new_block to true, since content inserted by plugins might omit
                        // the leading `-`.
                        parsed.new_block = true;
                        indent = parsed.indent;
                    } else if parsed.new_block && !in_code_block {
                        // Done with this block.
                        lines.put_back(Ok(line));
                        break;
                    }

                    if enter_code_block {
                        in_code_block = true;
                    }

                    if parsed.header > 0 {
                        header = parsed.header;
                    }

                    // Extract special attributes and omit them from the output
                    if parsed.attr_name == "id" {
                        id = parsed.attr_values.pop().unwrap_or_default();
                    } else if parsed.attr_name == "view-mode" {
                        view_type = parsed
                            .attr_values
                            .pop()
                            .map(ViewType::from)
                            .unwrap_or_default();
                    } else if parsed.attr_name == "logseq.order-list-type" {
                        let value = parsed.attr_values.pop().unwrap_or_default();
                        if value == "number" {
                            this_block_list_type = ListType::Number;
                        }
                    } else if parsed.attr_name == "collapsed" {
                        collapsed = parsed.attr_values.pop().unwrap_or_default() == "true";
                    } else {
                        if !parsed.attr_name.is_empty() {
                            attrs.insert(parsed.attr_name, parsed.attr_values);
                        }
                        line_contents.push(parsed.contents.to_string());
                    }
                }
            }
        } else {
            all_done = true;
            break;
        }
    }

    if line_contents.is_empty() {
        if all_done {
            return Ok(RawBlockOutput::Done);
        } else {
            return Ok(RawBlockOutput::Empty);
        }
    }

    let contents = line_contents.join("\n");
    let parsed = BlockContent::new_parsed(parse_string::ContentStyle::Logseq, contents)?;

    let mut tags = AttrList::new();
    for ex in parsed.borrow_parsed() {
        if let Expression::Hashtag(tag, _) = ex {
            tags.push(tag.to_string());
        }
    }

    let block_contents = LogseqRawBlock {
        id,
        header_level: header,
        // The caller will figure this out.
        parent_idx: None,
        view_type,
        this_block_list_type,
        collapsed,
        indent,
        contents: parsed,
        tags,
        attrs,
    };

    Ok(RawBlockOutput::Block(block_contents))
}

fn count_repeated_char(input: &str, match_char: char) -> IResult<&str, u32> {
    map(take_while(|c| c == match_char), |result: &str| {
        result.chars().count() as u32
    })(input)
}

fn space_between_tags(input: &str) -> IResult<&str, ()> {
    map(take_while1(|c| c != '#'), |_| ())(input)
}

fn evaluate_line(line: &str, in_code_block: bool) -> Result<Option<Line<'_>>> {
    if in_code_block {
        let line_without_tabs = line.trim_start_matches('\t');
        let output = if line_without_tabs.starts_with("  ") {
            // Trim off the leading whitespace of the code block too.
            &line_without_tabs[2..]
        } else {
            line
        };

        return Ok(Some(Line {
            contents: output,
            attr_name: String::new(),
            attr_values: SmallVec::new(),
            new_block: false,
            indent: 0,
            header: 0,
        }));
    }

    if line.is_empty() {
        return Ok(None);
    }

    let (rest, (indent, (dash, header))) = tuple((
        |input| count_repeated_char(input, '\t'),
        alt((
            map(
                tuple((
                    preceded(multispace0, tag("- ")),
                    opt(terminated(
                        |input| count_repeated_char(input, '#'),
                        tag(" "),
                    )),
                )),
                |(_, header_level)| (true, header_level.unwrap_or(0)),
            ),
            // Empty block
            map(all_consuming(tag("-")), |_| (true, 0)),
            // Normally there are two preceding spaces, but in some imported blocks
            // from Roam this is absent, so don't be strict about it.
            map(opt(tag("  ")), |_| (false, 0)),
        )),
    ))(line)
    .map_err(|e| eyre!("{}", e))?;

    let (attr_name, attr_values) = match parse_attr_line("::", rest) {
        Ok(Some(v)) => v,
        _ => (String::new(), SmallVec::new()),
    };

    Ok(Some(Line {
        contents: rest,
        indent,
        new_block: dash,
        header,
        attr_name,
        attr_values,
    }))
}

#[cfg(test)]
mod test {
    mod evaluate_line {
        use smallvec::{smallvec, SmallVec};

        use super::super::{evaluate_line, Line};

        #[test]
        fn empty_line() {
            let input = "";
            let result = evaluate_line(input, false).unwrap();
            assert!(result.is_none(), "Should be none: {result:?}");
        }

        #[test]
        fn no_indent_same_block() {
            let input = "  abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 0,
                    header: 0,
                    new_block: false,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn extra_spaces_same_block() {
            let input = "    abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "  abc",
                    indent: 0,
                    header: 0,
                    new_block: false,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn empty_block() {
            let input = "-";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "",
                    indent: 0,
                    header: 0,
                    new_block: true,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn indent_same_block() {
            let input = "\t\t  abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 2,
                    header: 0,
                    new_block: false,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn no_indent_new_block() {
            let input = "- abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 0,
                    header: 0,
                    new_block: true,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn indent_new_block() {
            let input = "\t\t- abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 2,
                    header: 0,
                    new_block: true,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn extra_space_before_dash() {
            let input = "\t\t - abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 2,
                    header: 0,
                    new_block: true,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn header1() {
            let input = "\t\t- # abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 2,
                    header: 1,
                    new_block: true,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn header3() {
            let input = "\t\t- ### abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 2,
                    header: 3,
                    new_block: true,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn hashes_on_second_line() {
            let input = "\t\t  ### abc";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "### abc",
                    indent: 2,
                    header: 0,
                    new_block: false,
                    attr_name: String::new(),
                    attr_values: SmallVec::new(),
                }
            );
        }

        #[test]
        fn new_block_attr_line() {
            let input = "\t\t- abc:: def";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc:: def",
                    indent: 2,
                    header: 0,
                    new_block: true,
                    attr_name: String::from("abc"),
                    attr_values: smallvec![String::from("def")],
                }
            );
        }

        #[test]
        fn same_block_attr_line() {
            let input = "\t\t  abc:: def";
            assert_eq!(
                evaluate_line(input, false).unwrap().unwrap(),
                Line {
                    contents: "abc:: def",
                    indent: 2,
                    header: 0,
                    new_block: false,
                    attr_name: String::from("abc"),
                    attr_values: smallvec![String::from("def")],
                }
            );
        }
    }
}
