use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take_until, take_while1},
    character::complete::{char, multispace0},
    character::{is_newline, is_space},
    combinator::{all_consuming, map, map_parser, opt, recognize},
    error::context,
    sequence::{delimited, pair, preceded, separated_pair, terminated, tuple},
    IResult,
};
use urlocator::{UrlLocation, UrlLocator};

#[derive(Debug, PartialEq, Eq)]
pub enum Expression<'a> {
    Text(&'a str),
    RawHyperlink(&'a str),
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
    Attribute {
        name: &'a str,
        value: Vec<Expression<'a>>,
    },
    Bold(Vec<Expression<'a>>),
    Italic(Vec<Expression<'a>>),
    Strike(Vec<Expression<'a>>),
    Highlight(Vec<Expression<'a>>),
    Latex(&'a str),
    BlockQuote(Vec<Expression<'a>>),
    HRule,
}

fn nonws_char(c: char) -> bool {
    !is_space(c as u8) && !is_newline(c as u8)
}

fn word(input: &str) -> IResult<&str, &str> {
    take_while1(nonws_char)(input)
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

fn latex(input: &str) -> IResult<&str, &str> {
    fenced("$$", "$$")(input)
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

/// Parses urls not inside a directive
fn raw_url(input: &str) -> IResult<&str, &str> {
    let mut locator = UrlLocator::new();
    let mut end = 0;
    for c in input.chars() {
        match locator.advance(c) {
            UrlLocation::Url(s, _e) => {
                end = s as usize;
            }
            UrlLocation::Reset => break,
            UrlLocation::Scheme => {}
        }
    }

    if end > 0 {
        Ok((&input[end..], &input[0..end]))
    } else {
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::RegexpFind,
        )))
    }
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
        map(context("bold", bold), Expression::Bold),
        map(italic, Expression::Italic),
        map(strike, Expression::Strike),
        map(highlight, Expression::Highlight),
        map(latex, Expression::Latex),
        map(raw_url, Expression::RawHyperlink),
    ))(input)
}

/// Parse a line of text, counting anything that doesn't match a directive as plain text.
fn parse_inline(input: &str) -> IResult<&str, Vec<Expression>> {
    let mut output = Vec::with_capacity(4);

    let mut current_input = input;

    while !current_input.is_empty() {
        let mut found_directive = false;
        for (current_index, _) in current_input.char_indices() {
            // println!("{} {}", current_index, current_input);
            match directive(&current_input[current_index..]) {
                Ok((remaining, parsed)) => {
                    // println!("Matched {:?} remaining {}", parsed, remaining);
                    let leading_text = &current_input[0..current_index];
                    if !leading_text.is_empty() {
                        output.push(Expression::Text(leading_text));
                    }
                    output.push(parsed);

                    current_input = remaining;
                    found_directive = true;
                    break;
                }
                Err(nom::Err::Error(_)) => {
                    // None of the parsers matched at the current position, so this character is just part of the text.
                    // The iterator will go to the next character so there's nothing to do here.
                }
                Err(e) => {
                    // On any other error, just return the error.
                    return Err(e);
                }
            }
        }

        if !found_directive {
            output.push(Expression::Text(current_input));
            break;
        }
    }

    Ok(("", output))
}

/// Parses `Name:: Arbitrary [[text]]`
fn attribute(input: &str) -> IResult<&str, (&str, Vec<Expression>)> {
    // Roam doesn't trim whitespace on the attribute name, so we don't either.
    separated_pair(is_not(":`"), tag("::"), preceded(multispace0, parse_inline))(input)
}

pub fn parse(input: &str) -> Result<Vec<Expression>, nom::Err<nom::error::Error<&str>>> {
    alt((
        map(all_consuming(tag("---")), |_| vec![Expression::HRule]),
        map(all_consuming(preceded(tag("> "), parse_inline)), |values| {
            vec![Expression::BlockQuote(values)]
        }),
        map(all_consuming(attribute), |(name, value)| {
            vec![Expression::Attribute { name, value }]
        }),
        all_consuming(parse_inline),
    ))(input)
    .map(|(_, results)| results)
}
