use nom::{
  branch::alt,
  bytes::complete::{is_not, tag},
  character::complete::{anychar, char, multispace0},
  combinator::{map, recognize},
  multi::{many0, many1},
  sequence::{delimited, preceded, terminated},
  IResult,
};

pub enum Expression<'a> {
  Word(&'a str),
  TripleBacktick(&'a str),
  SingleBacktick(&'a str),
  Hashtag(&'a str),
  Link(&'a str),
  BlockRef(&'a str),
}

fn triple_backtick(input: &str) -> IResult<&str, &str> {
  delimited(tag("```"), is_not("```"), tag("```"))(input)
}

fn single_backtick(input: &str) -> IResult<&str, &str> {
  delimited(char('`'), is_not("`"), char('`'))(input)
}

fn link(input: &str) -> IResult<&str, &str> {
  delimited(tag("[["), is_not("]]"), tag("]]"))(input)
}

fn block_ref(input: &str) -> IResult<&str, &str> {
  delimited(tag("(("), is_not("))"), tag("))"))(input)
}

fn word(input: &str) -> IResult<&str, &str> {
  terminated(recognize(many1(anychar)), multispace0)(input)
}

fn link_or_text(input: &str) -> IResult<&str, &str> {
  alt((link, word))(input)
}

fn hashtag(input: &str) -> IResult<&str, &str> {
  preceded(char('#'), link_or_text)(input)
}

fn parse_one(input: &str) -> IResult<&str, Expression> {
  alt((
    map(triple_backtick, Expression::TripleBacktick),
    map(single_backtick, Expression::SingleBacktick),
    map(hashtag, Expression::Hashtag),
    map(link, Expression::Link),
    map(block_ref, Expression::BlockRef),
    map(word, Expression::Word),
  ))(input)
}

pub fn parse(input: &str) -> IResult<&str, Vec<Expression>> {
  many0(parse_one)(input)
}
