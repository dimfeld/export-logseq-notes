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

use crate::graph::Block;

struct Line<'a> {
    contents: &'a str,
    indent: u32,
    new_block: bool,
}

// Take the first line separately just because that's how the header parser returns it.
pub fn parse_block(
    first_line: String,
    lines: &mut std::io::Lines<impl BufRead>,
) -> Result<(String, Option<Block>), anyhow::Error> {
    unimplemented!();

    if let Some(line) = evaulate_line(first_line.as_str()) {}
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

    let (rest, (indent, dash)) =
        tuple((count_indentation, opt(tag("- "))))(line).map_err(|e| anyhow!("{}", e))?;

    Ok(Some(Line {
        contents: rest,
        indent,
        new_block: dash.is_some(),
    }))
}
