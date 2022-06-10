use std::io::BufRead;

use anyhow::anyhow;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while},
    character::complete::multispace0,
    combinator::{all_consuming, map, opt},
    sequence::{preceded, terminated, tuple},
    IResult,
};
use smallvec::SmallVec;

use crate::graph::{AttrList, ViewType};

use super::{attrs::parse_attr_line, LinesIterator};

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
    pub contents: String,
    pub view_type: ViewType,
    pub indent: u32,
}

pub fn parse_raw_blocks(
    blocks: &mut Vec<LogseqRawBlock>,
    lines: &mut LinesIterator<impl BufRead>,
) -> Result<(), anyhow::Error> {
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

// Take the first line separately just because that's how the header parser returns it.
fn read_raw_block(
    lines: &mut LinesIterator<impl BufRead>,
) -> Result<RawBlockOutput, anyhow::Error> {
    // Most blocks will just be one or two lines
    let mut line_contents: SmallVec<[String; 2]> = SmallVec::new();
    let mut indent = 0;
    let mut id = String::new();
    let mut view_type = ViewType::Bullet;
    let mut header = 0;

    let mut all_done = false;

    loop {
        let line_read = lines.next();
        if let Some(line) = line_read {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            let parsed = evaluate_line(line.as_str())?;
            match parsed {
                None => break,
                Some(mut parsed) => {
                    if line_contents.is_empty() {
                        assert!(parsed.new_block, "{line} {parsed:?}");
                        indent = parsed.indent;
                    } else if parsed.new_block {
                        // Done with this block.
                        lines.put_back(Ok(line));
                        break;
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
                    } else {
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

    let block_contents = LogseqRawBlock {
        id,
        header_level: header,
        // The caller will figure this out.
        parent_idx: None,
        view_type,
        indent,
        contents: line_contents.join("\n"),
    };

    Ok(RawBlockOutput::Block(block_contents))
}

fn count_repeated_char(input: &str, match_char: char) -> IResult<&str, u32> {
    map(take_while(|c| c == match_char), |result: &str| {
        result.chars().count() as u32
    })(input)
}

fn evaluate_line(line: &str) -> Result<Option<Line<'_>>, anyhow::Error> {
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
    .map_err(|e| anyhow!("{}", e))?;

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
            let result = evaluate_line(input).unwrap();
            assert!(result.is_none(), "Should be none: {:?}", result);
        }

        #[test]
        fn no_indent_same_block() {
            let input = "  abc";
            assert_eq!(
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
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
                evaluate_line(input).unwrap().unwrap(),
                Line {
                    contents: "abc:: def",
                    indent: 2,
                    header: 0,
                    new_block: true,
                    attr_name: String::from("abc"),
                    attr_values: smallvec![String::from("def")]
                }
            );
        }

        #[test]
        fn same_block_attr_line() {
            let input = "\t\t  abc:: def";
            assert_eq!(
                evaluate_line(input).unwrap().unwrap(),
                Line {
                    contents: "abc:: def",
                    indent: 2,
                    header: 0,
                    new_block: false,
                    attr_name: String::from("abc"),
                    attr_values: smallvec![String::from("def")]
                }
            );
        }
    }
}
