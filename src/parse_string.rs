use nom::{
  branch::alt,
  bytes::complete::{is_a, is_not, tag, take_until, take_while1},
  character::complete::{anychar, char, line_ending, multispace0, one_of},
  character::{is_newline, is_space},
  combinator::{all_consuming, eof, map, map_parser, not, opt, peek, recognize, value},
  multi::{many0, many1, many_till},
  sequence::{delimited, pair, preceded, separated_pair, terminated, tuple},
  IResult,
};
use std::borrow::Cow;

// TODO Parse attributes

#[derive(Debug, PartialEq, Eq)]
pub enum Expression<'a> {
  Text(&'a str),
  Image {
    alt: &'a str,
    url: &'a str,
  },
  BraceDirective(&'a str),
  Table,
  PageEmbed(&'a str),
  BlockEmbed(&'a str),
  TripleBacktick(&'a str),
  SingleBacktick(&'a str),
  Hashtag(&'a str, bool),
  Link(&'a str),
  MarkdownLink {
    title: &'a str,
    url: &'a str,
  },
  BlockRef(&'a str),
  // Use a box here to prevent rust complaining about infinite recursion
  Attribute {
    name: &'a str,
    value: Vec<Expression<'a>>,
  },
  Bold(Vec<Expression<'a>>),
  Italic(Vec<Expression<'a>>),
  Strike(Vec<Expression<'a>>),
  Highlight(Vec<Expression<'a>>),
  HRule,
}

impl<'a> Expression<'a> {
  pub fn plaintext(&self) -> &'a str {
    match self {
      Expression::Text(s) => s,
      Expression::SingleBacktick(s) => s,
      Expression::Hashtag(s, _) => s,
      Expression::Link(s) => s,
      Expression::MarkdownLink { title, .. } => title,
      Expression::BlockRef(s) => s,
      _ => "",
    }
  }
}

// fn ws(s: &str) -> IResult<&str, &str> {
//   alt((one_of(" \t\r\n"), eof))(s)
// }

fn nonws_char(c: char) -> bool {
  !is_space(c as u8) && !is_newline(c as u8)
}

fn ws_char(c: char) -> bool {
  is_space(c as u8) || is_newline(c as u8)
}

fn whitespace(input: &str) -> IResult<&str, &str> {
  take_while1(ws_char)(input)
}

fn start_of_directive(input: &str) -> IResult<&str, &str> {
  alt((tag("{{"), tag("[["), tag("#"), tag("`"), tag("![")))(input)
}

fn text(input: &str) -> IResult<&str, &str> {
  alt((
    recognize(delimited(multispace0, is_not("#`[{!"), multispace0)),
    whitespace,
  ))(input)
}

fn directive_headfakes(input: &str) -> IResult<&str, &str> {
  recognize(preceded(one_of("!{["), alt((is_not("{[!#"), eof))))(input)
}

fn word(input: &str) -> IResult<&str, &str> {
  recognize(take_while1(nonws_char))(input)
}

fn fenced<'a>(start: &'a str, end: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
  map(tuple((tag(start), take_until(end), tag(end))), |x| x.1)
}

fn style<'a>(boundary: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Expression<'a>>> {
  map_parser(fenced(boundary, boundary), parse_inline)
}

fn link(input: &str) -> IResult<&str, &str> {
  fenced("[[", "]]")(input)
}

fn markdown_link(input: &str) -> IResult<&str, (&str, &str)> {
  pair(
    fenced("[", "]"),
    delimited(char('('), is_not(")"), char(')')),
  )(input)
}

fn link_or_word(input: &str) -> IResult<&str, &str> {
  alt((link, word))(input)
}

fn fixed_link_or_word<'a>(word: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
  alt((tag(word), delimited(tag("[["), tag(word), tag("]]"))))
}

fn hashtag(input: &str) -> IResult<&str, (&str, bool)> {
  map(
    preceded(char('#'), pair(opt(tag(".")), link_or_word)),
    |(has_dot, tag)| (tag, has_dot.is_some()),
  )(input)
}

fn triple_backtick(input: &str) -> IResult<&str, &str> {
  fenced("```", "```")(input)
}

fn single_backtick(input: &str) -> IResult<&str, &str> {
  delimited(char('`'), is_not("`"), char('`'))(input)
}

// Parse `((refrence))`
fn block_ref(input: &str) -> IResult<&str, &str> {
  fenced("((", "))")(input)
}

fn bold(input: &str) -> IResult<&str, Vec<Expression>> {
  style("**")(input)
}

fn italic(input: &str) -> IResult<&str, Vec<Expression>> {
  style("__")(input)
}

fn strike(input: &str) -> IResult<&str, Vec<Expression>> {
  style("~~")(input)
}

fn highlight(input: &str) -> IResult<&str, Vec<Expression>> {
  style("^^")(input)
}

fn brace_directive_contents(input: &str) -> IResult<&str, Expression> {
  alt((
    map(fixed_link_or_word("table"), |_| Expression::Table),
    map(
      separated_pair(
        fixed_link_or_word("embed"),
        terminated(tag(":"), multispace0),
        alt((
          map(block_ref, Expression::BlockEmbed),
          map(link, Expression::PageEmbed),
        )),
      ),
      |(_, e)| e,
    ),
    map(link_or_word, Expression::BraceDirective),
  ))(input)
}

/// Parse directives like `{{table}}` and `{{[[table]]}}`
fn brace_directive(input: &str) -> IResult<&str, Expression> {
  map(
    tuple((
      tag("{{"),
      map(take_until("}}"), |inner: &str| {
        // Try to parse a link from the brace contents. If these fail, just return the raw token.
        let inner = inner.trim();
        all_consuming(brace_directive_contents)(inner)
          .map(|x| x.1)
          .unwrap_or_else(|_| Expression::BraceDirective(inner))
      }),
      tag("}}"),
    )),
    |x| x.1,
  )(input)
}

/// Parses `![alt](url)`
fn image(input: &str) -> IResult<&str, (&str, &str)> {
  preceded(char('!'), markdown_link)(input)
}

fn directive(input: &str) -> IResult<&str, Expression> {
  alt((
    map(triple_backtick, Expression::TripleBacktick),
    map(single_backtick, Expression::SingleBacktick),
    brace_directive,
    map(hashtag, |(v, dot)| Expression::Hashtag(v, dot)),
    map(link, Expression::Link),
    map(block_ref, Expression::BlockRef),
    map(image, |(alt, url)| Expression::Image { alt, url }),
    map(markdown_link, |(title, url)| Expression::MarkdownLink {
      title,
      url,
    }),
    map(bold, Expression::Bold),
    map(italic, Expression::Italic),
    map(strike, Expression::Strike),
    map(highlight, Expression::Highlight),
  ))(input)
}

fn parse_one(input: &str) -> IResult<&str, Expression> {
  // TODO I think a better solution would be to remove "text" from the parser
  // and just step it through the string until it finds a directive. Then
  // put all the previous text into an Expression::Text and return the directive as well.
  alt((
    directive,
    map(directive_headfakes, Expression::Text),
    map(text, Expression::Text),
  ))(input)
}

fn parse_inline(input: &str) -> IResult<&str, Vec<Expression>> {
  many0(parse_one)(input)
}

/// Parses `Name:: Arbitrary [[text]]`
fn attribute(input: &str) -> IResult<&str, (&str, Vec<Expression>)> {
  // Roam doesn't trim whitespace on the attribute name, so we don't either.
  separated_pair(is_not(":`"), tag("::"), preceded(multispace0, parse_inline))(input)
}

pub fn parse(input: &str) -> Result<Vec<Expression>, nom::Err<nom::error::Error<&str>>> {
  all_consuming(alt((
    map(tag("---"), |_| vec![Expression::HRule]),
    map(attribute, |(name, value)| {
      vec![Expression::Attribute { name, value }]
    }),
    parse_inline,
  )))(input)
  .map(|(_, results)| results)
}
