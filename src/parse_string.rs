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
  MarkdownLink { title: &'a str, url: &'a str },
  BlockRef(&'a str),
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
  recognize(preceded(one_of("{["), alt((is_not("{[!#"), eof))))(input)
}

fn word(input: &str) -> IResult<&str, &str> {
  recognize(take_while1(nonws_char))(input)
}

fn fenced<'a>(start: &'a str, end: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
  map(tuple((tag(start), take_until(end), tag(end))), |x| x.1)
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

fn hashtag(input: &str) -> IResult<&str, &str> {
  preceded(char('#'), link_or_word)(input)
}

fn triple_backtick(input: &str) -> IResult<&str, &str> {
  fenced("```", "```")(input)
}

fn single_backtick(input: &str) -> IResult<&str, &str> {
  delimited(char('`'), is_not("`"), char('`'))(input)
}

fn block_ref(input: &str) -> IResult<&str, &str> {
  fenced("((", "))")(input)
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
  preceded(char('!'), markdown_link)(input)
}

fn directive(input: &str) -> IResult<&str, Expression> {
  alt((
    map(triple_backtick, Expression::TripleBacktick),
    map(single_backtick, Expression::SingleBacktick),
    map(brace_directive, Expression::BraceDirective),
    map(hashtag, Expression::Hashtag),
    map(link, Expression::Link),
    map(block_ref, Expression::BlockRef),
    map(image, |(alt, url)| Expression::Image { alt, url }),
    map(markdown_link, |(title, url)| Expression::MarkdownLink {
      title,
      url,
    }),
  ))(input)
}

fn parse_one(input: &str) -> IResult<&str, Expression> {
  // I think a better solution would be to remove "text" from the parser
  // and just step it through the string until it finds a directive. Then
  // put all the previous text into an Expression::Text and return the directive as well.
  alt((
    directive,
    map(directive_headfakes, Expression::Text),
    map(text, Expression::Text),
  ))(input)
}

pub fn parse(input: &str) -> Result<Vec<Expression>, nom::Err<nom::error::Error<&str>>> {
  all_consuming(many0(parse_one))(input).map(|(_, results)| results)
}
