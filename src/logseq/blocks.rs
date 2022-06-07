use std::io::BufRead;

use anyhow::anyhow;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while},
    combinator::{map, opt},
    multi::many0,
    sequence::tuple,
    IResult,
};
use smallvec::SmallVec;

use crate::graph::Block;

use super::LinesIterator;

#[derive(Debug, PartialEq, Eq)]
struct Line<'a> {
    contents: &'a str,
    indent: u32,
    new_block: bool,
}

struct LogseqRawBlock {
    contents: String,
    indent: u32,
}

pub fn parse_blocks(lines: &mut LinesIterator<impl BufRead>) -> Result<Vec<Block>, anyhow::Error> {
    todo!();
}

fn parse_raw_blocks(
    lines: &mut LinesIterator<impl BufRead>,
) -> Result<Vec<LogseqRawBlock>, anyhow::Error> {
    let mut blocks = Vec::new();

    loop {
        match read_raw_block(lines)? {
            RawBlockOutput::Done => break,
            RawBlockOutput::Empty => {}
            RawBlockOutput::Block(block) => {
                blocks.push(block);
            }
        }
    }

    Ok(blocks)
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

    loop {
        let line_read = lines.next();
        if let Some(line) = line_read {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            let parsed = evaulate_line(line.as_str())?;
            match parsed {
                None => break,
                Some(parsed) => {
                    if line_contents.is_empty() {
                        assert!(parsed.new_block);
                        indent = parsed.indent;
                    } else if parsed.new_block {
                        // Done with this block.
                        lines.put_back(Ok(line));
                        break;
                    }

                    line_contents.push(parsed.contents.to_string());
                }
            }
        } else {
            return Ok(RawBlockOutput::Done);
        }
    }

    if line_contents.is_empty() {
        return Ok(RawBlockOutput::Empty);
    }

    let block_contents = LogseqRawBlock {
        contents: line_contents.join("\n"),
        indent,
    };

    Ok(RawBlockOutput::Block(block_contents))
}

fn count_indentation(input: &str) -> IResult<&str, u32> {
    map(take_while(|c| c == '\t'), |tabs: &str| {
        tabs.chars().count() as u32
    })(input)
}

fn evaulate_line<'a>(line: &'a str) -> Result<Option<Line<'a>>, anyhow::Error> {
    if line.is_empty() {
        return Ok(None);
    }

    let (rest, (indent, dash)) = tuple((
        count_indentation,
        alt((map(tag("- "), |_| true), map(tag("  "), |_| false))),
    ))(line)
    .map_err(|e| anyhow!("{}", e))?;

    Ok(Some(Line {
        contents: rest,
        indent,
        new_block: dash,
    }))
}

#[cfg(test)]
mod test {
    mod evaulate_line {
        use super::super::{evaulate_line, Line};

        #[test]
        fn empty_line() {
            let input = "";
            let result = evaulate_line(input).unwrap();
            assert!(result.is_none(), "Should be none: {:?}", result);
        }

        #[test]
        fn no_indent_same_block() {
            let input = "  abc";
            assert_eq!(
                evaulate_line(input).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 0,
                    new_block: false
                }
            );
        }

        #[test]
        fn indent_same_block() {
            let input = "\t\t  abc";
            assert_eq!(
                evaulate_line(input).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 2,
                    new_block: false
                }
            );
        }

        #[test]
        fn no_indent_new_block() {
            let input = "- abc";
            assert_eq!(
                evaulate_line(input).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 0,
                    new_block: true
                }
            );
        }

        #[test]
        fn indent_new_block() {
            let input = "\t\t- abc";
            assert_eq!(
                evaulate_line(input).unwrap().unwrap(),
                Line {
                    contents: "abc",
                    indent: 2,
                    new_block: true
                }
            );
        }
    }
}
