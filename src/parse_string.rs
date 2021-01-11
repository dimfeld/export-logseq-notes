use nom::{
  branch::alt,
  bytes::complete::{is_not, tag},
  character::complete::{anychar, char, line_ending, multispace0},
  combinator::{map, map_parser, recognize},
  multi::{many0, many1},
  sequence::{delimited, preceded, terminated},
  IResult,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Expression<'a> {
  Text(&'a str),
  BraceDirective(&'a str),
  TripleBacktick(&'a str),
  SingleBacktick(&'a str),
  Hashtag(&'a str),
  Link(&'a str),
  BlockRef(&'a str),
}

fn not_space(s: &str) -> IResult<&str, &str> {
  is_not(" \t\r\n")(s)
}

fn start_of_directive(input: &str) -> IResult<&str, &str> {
  alt((tag("{{"), tag("[["), tag("#"), tag("`")))(input)
}

fn text(input: &str) -> IResult<&str, &str> {
  recognize(delimited(
    multispace0,
    recognize(many1(is_not("{[#`"))),
    multispace0,
  ))(input)
}

fn link(input: &str) -> IResult<&str, &str> {
  delimited(tag("[["), is_not("]]"), tag("]]"))(input)
}

fn link_or_text(input: &str) -> IResult<&str, &str> {
  alt((link, text))(input)
}

fn hashtag(input: &str) -> IResult<&str, &str> {
  preceded(char('#'), link_or_text)(input)
}

fn link_or_text_or_hashtag(input: &str) -> IResult<&str, &str> {
  alt((hashtag, link, text))(input)
}

fn triple_backtick(input: &str) -> IResult<&str, &str> {
  delimited(tag("```"), is_not("```"), tag("```"))(input)
}

fn single_backtick(input: &str) -> IResult<&str, &str> {
  delimited(char('`'), is_not("`"), char('`'))(input)
}

fn block_ref(input: &str) -> IResult<&str, &str> {
  delimited(tag("(("), is_not("))"), tag("))"))(input)
}

fn brace_directive(input: &str) -> IResult<&str, &str> {
  delimited(
    tag("{{"),
    map_parser(is_not("}}"), link_or_text_or_hashtag),
    tag("}}"),
  )(input)
}

fn parse_one(input: &str) -> IResult<&str, Expression> {
  alt((
    map(triple_backtick, Expression::TripleBacktick),
    map(single_backtick, Expression::SingleBacktick),
    map(brace_directive, Expression::BraceDirective),
    map(hashtag, Expression::Hashtag),
    map(link, Expression::Link),
    map(block_ref, Expression::BlockRef),
    map(text, Expression::Text),
  ))(input)
}

pub fn parse(input: &str) -> Result<Vec<Expression>, nom::Err<nom::error::Error<&str>>> {
  many0(parse_one)(input).map(|(_, e)| e)
}

#[test]
fn test_word() {
  let input = "word";
  assert_eq!(parse(input).unwrap(), vec![Expression::Text("word")])
}

#[test]
fn test_words() {
  let input = "two words";
  assert_eq!(parse(input).unwrap(), vec![Expression::Text("two words")])
}

#[test]
fn test_surrounding_whitespace() {
  let input = "  two words  ";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Text("  two words  ")]
  )
}

#[test]
fn test_block_ref() {
  let input = "((a ref))";
  assert_eq!(parse(input).unwrap(), vec![Expression::BlockRef("a ref")])
}

#[test]
fn test_link() {
  let input = "[[a title]]";
  assert_eq!(parse(input).unwrap(), vec![Expression::Link("a title")])
}

#[test]
fn test_hashtag_simple() {
  let input = "#tag";
  assert_eq!(parse(input).unwrap(), vec![Expression::Hashtag("tag")])
}

#[test]
fn test_hashtag_with_link() {
  let input = "#[[a tag]]";
  assert_eq!(parse(input).unwrap(), vec![Expression::Hashtag("a tag")])
}

#[test]
fn test_simple_brace() {
  let input = "{{table}}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("table")]
  )
}

#[test]
fn test_hashtag_brace() {
  let input = "{{#table}}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("table")]
  )
}

#[test]
fn test_link_brace() {
  let input = "{{[[table]]}}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("table")]
  )
}

#[test]
fn test_multiword_with_links() {
  let input = "I want an [[astrolabe]] of my own";
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("I want an "),
      Expression::Link("astrolabe"),
      Expression::Text(" of my own")
    ]
  )
}
