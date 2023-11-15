use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take_until, take_while1},
    character::{
        complete::{char, multispace0, multispace1},
        is_newline,
    },
    combinator::{all_consuming, cond, map, map_opt, map_parser, opt},
    error::context,
    sequence::{delimited, pair, preceded, separated_pair, terminated, tuple},
    IResult,
};
use urlocator::{UrlLocation, UrlLocator};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentStyle {
    Roam,
    Logseq,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Expression<'a> {
    Text(&'a str),
    RawHtml(&'a str),
    RawHyperlink(&'a str),
    Image {
        alt: &'a str,
        url: &'a str,
    },
    Video {
        url: &'a str,
    },
    BraceDirective(&'a str),
    Table,
    Todo {
        done: bool,
    },
    PageEmbed(&'a str),
    BlockEmbed(&'a str),
    TripleBacktick(&'a str),
    SingleBacktick(&'a str),
    Hashtag(&'a str, bool),
    Link(&'a str),
    MarkdownInternalLink {
        label: &'a str,
        page: &'a str,
    },
    MarkdownExternalLink {
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

impl<'a> Expression<'a> {
    pub fn contained_expressions(&self) -> &[Expression<'a>] {
        match self {
            Expression::Bold(exprs) => exprs,
            Expression::Italic(exprs) => exprs,
            Expression::Strike(exprs) => exprs,
            Expression::Highlight(exprs) => exprs,
            Expression::BlockQuote(exprs) => exprs,
            Expression::Attribute { value, .. } => value,
            _ => &[],
        }
    }
}

/// Take a string delimited by some characters, but track how many times the delimiter pairs
/// themselves also appear in the string.
/// From https://gitlab.com/getreu/parse-hyperlinks/-/blob/master/parse-hyperlinks/src/lib.rs
fn take_until_unbalanced(
    opening_bracket: char,
    closing_bracket: char,
) -> impl Fn(&str) -> IResult<&str, &str> {
    move |i: &str| {
        let mut index = 0;
        let mut bracket_counter = 0;
        while let Some(n) = &i[index..].find(&[opening_bracket, closing_bracket, '\\'][..]) {
            index += n;
            let mut it = i[index..].chars();
            match it.next().unwrap_or_default() {
                c if c == '\\' => {
                    // Skip the escape char `\`.
                    index += '\\'.len_utf8();
                    // Skip also the following char.
                    let c = it.next().unwrap_or_default();
                    index += c.len_utf8();
                }
                c if c == opening_bracket => {
                    bracket_counter += 1;
                    index += opening_bracket.len_utf8();
                }
                c if c == closing_bracket => {
                    // Closing bracket.
                    bracket_counter -= 1;
                    index += closing_bracket.len_utf8();
                }
                // Can not happen.
                _ => unreachable!(),
            };
            // We found the unmatched closing bracket.
            if bracket_counter == -1 {
                // We do not consume it.
                index -= closing_bracket.len_utf8();
                return Ok((&i[index..], &i[0..index]));
            };
        }

        if bracket_counter == 0 {
            Ok(("", i))
        } else {
            Err(nom::Err::Error(nom::error::Error::new(
                i,
                nom::error::ErrorKind::TakeUntil,
            )))
        }
    }
}

fn nonws_char(c: char) -> bool {
    !c.is_whitespace() && !is_newline(c as u8)
}

fn word(input: &str) -> IResult<&str, &str> {
    take_while1(|c| nonws_char(c) && c != ',')(input)
}

fn fenced<'a>(start: &'a str, end: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    map(tuple((tag(start), take_until(end), tag(end))), |x| x.1)
}

fn style<'a>(
    content_style: ContentStyle,
    boundary: &'a str,
) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Expression<'a>>> {
    map_parser(fenced(boundary, boundary), move |i| {
        parse_inline(content_style, false, i)
    })
}

fn link(input: &str) -> IResult<&str, &str> {
    fenced("[[", "]]")(input)
}

fn markdown_link(input: &str) -> IResult<&str, (&str, &str)> {
    pair(
        fenced("[", "]"),
        delimited(char('('), take_until_unbalanced('(', ')'), char(')')),
    )(input)
}

pub fn link_or_word(input: &str) -> IResult<&str, &str> {
    alt((link, word))(input)
}

fn fixed_link_or_word<'a>(word: &'a str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    alt((tag(word), delimited(tag("[["), tag(word), tag("]]"))))
}

pub fn hashtag(input: &str) -> IResult<&str, (&str, bool)> {
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

fn roam_bold(content_style: ContentStyle, input: &str) -> IResult<&str, Vec<Expression>> {
    style(content_style, "**")(input)
}

fn logseq_bold(content_style: ContentStyle, input: &str) -> IResult<&str, Vec<Expression>> {
    alt((style(content_style, "**"), style(content_style, "__")))(input)
}

fn roam_italic(content_style: ContentStyle, input: &str) -> IResult<&str, Vec<Expression>> {
    style(content_style, "__")(input)
}

fn logseq_italic(content_style: ContentStyle, input: &str) -> IResult<&str, Vec<Expression>> {
    alt((style(content_style, "_"), style(content_style, "*")))(input)
}

fn strike(content_style: ContentStyle, input: &str) -> IResult<&str, Vec<Expression>> {
    style(content_style, "~~")(input)
}

fn highlight(content_style: ContentStyle, input: &str) -> IResult<&str, Vec<Expression>> {
    style(content_style, "^^")(input)
}

fn latex(input: &str) -> IResult<&str, &str> {
    fenced("$$", "$$")(input)
}

fn brace_directive_contents(content_style: ContentStyle, input: &str) -> IResult<&str, Expression> {
    alt((
        map_opt(
            cond(
                content_style == ContentStyle::Roam,
                alt((
                    map(fixed_link_or_word("TODO"), |_| Expression::Todo {
                        done: false,
                    }),
                    map(fixed_link_or_word("DOING"), |_| Expression::Todo {
                        done: false,
                    }),
                    map(fixed_link_or_word("DONE"), |_| Expression::Todo {
                        done: true,
                    }),
                )),
            ),
            |r| r,
        ),
        map(fixed_link_or_word("table"), |_| Expression::Table),
        map(
            separated_pair(fixed_link_or_word("video"), multispace1, raw_url),
            |(_, url)| Expression::Video { url },
        ),
        map(
            separated_pair(
                fixed_link_or_word("embed"),
                // Roam has a colon after "embed", Logseq does not.
                alt((
                    map_opt(
                        cond(
                            content_style == ContentStyle::Roam,
                            terminated(tag(":"), multispace0),
                        ),
                        |r| r,
                    ),
                    map_opt(
                        cond(content_style == ContentStyle::Logseq, multispace1),
                        |r| r,
                    ),
                )),
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
fn brace_directive(content_style: ContentStyle, input: &str) -> IResult<&str, Expression> {
    map(
        tuple((
            tag("{{"),
            map(take_until("}}"), |inner: &str| {
                // Try to parse a link from the brace contents. If these fail, just return the raw token.
                let inner = inner.trim();
                all_consuming(|i| brace_directive_contents(content_style, i))(inner)
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

fn raw_html(input: &str) -> IResult<&str, &str> {
    fenced("@@html: ", "@@")(input)
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

fn directive(
    content_style: ContentStyle,
    allow_attribute: bool,
    input: &str,
) -> IResult<&str, Expression> {
    alt((
        map(triple_backtick, Expression::TripleBacktick),
        map(single_backtick, Expression::SingleBacktick),
        |i| brace_directive(content_style, i),
        map(hashtag, |(v, dot)| Expression::Hashtag(v, dot)),
        map(link, Expression::Link),
        map(block_ref, Expression::BlockRef),
        map(image, |(alt, url)| Expression::Image { alt, url }),
        map(raw_html, Expression::RawHtml),
        map(markdown_link, |(title, url)| {
            if let Ok((_, url)) = (all_consuming(link))(url) {
                Expression::MarkdownInternalLink {
                    label: title,
                    page: url,
                }
            } else {
                Expression::MarkdownExternalLink { title, url }
            }
        }),
        map_opt(
            cond(
                content_style == ContentStyle::Roam,
                alt((
                    map(
                        context("bold", |i| roam_bold(content_style, i)),
                        Expression::Bold,
                    ),
                    map(|i| roam_italic(content_style, i), Expression::Italic),
                )),
            ),
            |r| r,
        ),
        map_opt(
            cond(
                content_style == ContentStyle::Logseq,
                alt((
                    map(
                        context("bold", |i| logseq_bold(content_style, i)),
                        Expression::Bold,
                    ),
                    map(|i| logseq_italic(content_style, i), Expression::Italic),
                )),
            ),
            |r| r,
        ),
        map(|i| strike(content_style, i), Expression::Strike),
        map(|i| highlight(content_style, i), Expression::Highlight),
        map(latex, Expression::Latex),
        map(raw_url, Expression::RawHyperlink),
        map_opt(
            cond(
                allow_attribute,
                map(
                    |i| attribute(content_style, i),
                    |(name, value)| Expression::Attribute { name, value },
                ),
            ),
            |r| r,
        ),
    ))(input)
}

/// Parse a line of text, counting anything that doesn't match a directive as plain text.
fn parse_inline(
    style: ContentStyle,
    in_attribute: bool,
    input: &str,
) -> IResult<&str, Vec<Expression>> {
    let mut output = Vec::with_capacity(4);

    let mut current_input = input;

    while !current_input.is_empty() {
        let mut found_directive = false;
        for (current_index, _) in current_input.char_indices() {
            // println!("{} {}", current_index, current_input);
            match directive(style, in_attribute, &current_input[current_index..]) {
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
pub fn attribute(style: ContentStyle, input: &str) -> IResult<&str, (&str, Vec<Expression>)> {
    // Roam doesn't trim whitespace on the attribute name, so we don't either.
    match style {
        ContentStyle::Roam => separated_pair(
            is_not(":`"),
            tag("::"),
            preceded(multispace0, |i| parse_inline(style, false, i)),
        )(input),
        ContentStyle::Logseq => separated_pair(
            preceded(
                multispace0,
                take_while1(|c| nonws_char(c) && c != ',' && c != ':'),
            ),
            tag(":: "),
            preceded(multispace0, |i| parse_inline(style, false, i)),
        )(input),
    }
}

fn logseq_todo(input: &str) -> IResult<&str, Expression> {
    alt((
        map(tag("TODO"), |_| Expression::Todo { done: false }),
        map(tag("DOING"), |_| Expression::Todo { done: false }),
        map(tag("NOW"), |_| Expression::Todo { done: false }),
        map(tag("LATER"), |_| Expression::Todo { done: false }),
        map(tag("DONE"), |_| Expression::Todo { done: true }),
    ))(input)
}

pub fn parse<'a>(
    content_style: ContentStyle,
    input: &'a str,
) -> Result<Vec<Expression<'a>>, nom::Err<nom::error::Error<&'a str>>> {
    alt((
        map(all_consuming(tag("---")), |_| vec![Expression::HRule]),
        map(
            all_consuming(preceded(tag("> "), |i| {
                parse_inline(content_style, true, i)
            })),
            |values| vec![Expression::BlockQuote(values)],
        ),
        map_opt(
            cond(
                content_style == ContentStyle::Roam,
                map(
                    all_consuming(|i| attribute(content_style, i)),
                    |(name, value)| vec![Expression::Attribute { name, value }],
                ),
            ),
            |r| r,
        ),
        map_opt(
            cond(
                content_style == ContentStyle::Logseq,
                all_consuming(map(
                    pair(logseq_todo, |i| parse_inline(content_style, true, i)),
                    |(todo_expr, mut exprs)| {
                        exprs.insert(0, todo_expr);
                        exprs
                    },
                )),
            ),
            |r| r,
        ),
        all_consuming(|input| parse_inline(content_style, true, input)),
    ))(input)
    .map(|(_, results)| results)
}
