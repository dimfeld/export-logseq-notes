use nom::{
  branch::alt,
  bytes::complete::{is_a, is_not, tag, take_until, take_while1},
  character::complete::{anychar, char, line_ending, multispace0, one_of},
  character::{is_newline, is_space},
  combinator::{all_consuming, eof, map, map_parser, not, peek, recognize, value},
  multi::{many0, many1, many_till},
  sequence::{delimited, pair, preceded, terminated, tuple},
  IResult,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Expression<'a> {
  Text(&'a str),
  Image { alt: &'a str, url: &'a str },
  BraceDirective(&'a str),
  TripleBacktick(&'a str),
  SingleBacktick(&'a str),
  Hashtag(&'a str),
  Link(&'a str),
  BlockRef(&'a str),
}

// fn ws(s: &str) -> IResult<&str, &str> {
//   alt((one_of(" \t\r\n"), eof))(s)
// }

fn nonws_char(c: char) -> bool {
  !is_space(c as u8) && !is_newline(c as u8)
}

fn start_of_directive(input: &str) -> IResult<&str, &str> {
  alt((tag("{{"), tag("[["), tag("#"), tag("`"), tag("![")))(input)
}

fn text(input: &str) -> IResult<&str, &str> {
  recognize(delimited(multispace0, is_not("[{!\t\r\n"), multispace0))(input)
}

fn directive_headfakes(input: &str) -> IResult<&str, &str> {
  recognize(alt((
    preceded(tag("{"), alt((is_not("{"), eof))),
    preceded(tag("["), alt((is_not("["), eof))),
  )))(input)
}

fn word(input: &str) -> IResult<&str, &str> {
  recognize(take_while1(nonws_char))(input)
}

fn fenced<'a>(start: &str, input: &'a str, end: &str) -> IResult<&'a str, &'a str> {
  map(tuple((tag(start), take_until(end), tag(end))), |x| x.1)(input)
}

fn link(input: &str) -> IResult<&str, &str> {
  fenced("[[", input, "]]")
}

fn link_or_word(input: &str) -> IResult<&str, &str> {
  alt((link, word))(input)
}

fn hashtag(input: &str) -> IResult<&str, &str> {
  preceded(char('#'), link_or_word)(input)
}

fn triple_backtick(input: &str) -> IResult<&str, &str> {
  fenced("```", input, "```")
}

fn single_backtick(input: &str) -> IResult<&str, &str> {
  delimited(char('`'), is_not("`"), char('`'))(input)
}

fn block_ref(input: &str) -> IResult<&str, &str> {
  fenced("((", input, "))")
}

fn brace_directive(input: &str) -> IResult<&str, &str> {
  map(
    tuple((
      tag("{{"),
      map(take_until("}}"), |inner| {
        // Try to parse a link from the brace contents. If these fail, just return the raw token.
        all_consuming(link)(inner)
          .map(|x| x.1)
          .unwrap_or_else(|_| inner.trim())
      }),
      tag("}}"),
    )),
    |x| x.1,
  )(input)
}

fn image(input: &str) -> IResult<&str, (&str, &str)> {
  preceded(
    char('!'),
    pair(
      delimited(char('['), is_not("]"), char(']')),
      delimited(char('('), is_not(")"), char(')')),
    ),
  )(input)
}

fn directive(input: &str) -> IResult<&str, Expression> {
  alt((
    map(triple_backtick, Expression::TripleBacktick),
    map(single_backtick, Expression::SingleBacktick),
    map(image, |(alt, url)| Expression::Image { alt, url }),
    map(brace_directive, Expression::BraceDirective),
    map(hashtag, Expression::Hashtag),
    map(link, Expression::Link),
    map(block_ref, Expression::BlockRef),
  ))(input)
}

fn parse_one(input: &str) -> IResult<&str, Expression> {
  alt((
    directive,
    map(text, Expression::Text),
    map(directive_headfakes, Expression::Text),
  ))(input)
}

pub fn parse(input: &str) -> Result<Vec<Expression>, nom::Err<nom::error::Error<&str>>> {
  all_consuming(many0(parse_one))(input).map(|(_, results)| results)
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
  let input = "{{ table }}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("table")]
  )
}

#[test]
fn test_hashtag_brace() {
  // This isn't valid in Roam, so it doesn't parse out the hashtag.
  let input = "{{ #table}}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("#table")]
  )
}

#[test]
fn test_link_with_enclosed_bracket() {
  let input = "[[ab[cd]ef]]";
  assert_eq!(parse(input).unwrap(), vec![Expression::Link("ab[cd]ef")])
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

// Test[[link]]in a word
// Test#hashtag in a word

#[test]
fn test_single_brace() {
  let input = "this is not [a brace ] but [[this is]]";
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("this is not "),
      Expression::Text("[a brace ] but "),
      Expression::Link("this is")
    ]
  )
}

#[test]
fn test_single_bracket() {
  let input = "this is not {a bracket } but {{this is}}";
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("this is not "),
      Expression::Text("{a bracket } but "),
      Expression::BraceDirective("this is")
    ]
  )
}
