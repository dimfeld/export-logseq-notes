use anyhow::anyhow;
use fxhash::FxHashMap;
use nom::{
    branch::alt, bytes::complete::take_while1, character::is_space, combinator::map,
    multi::separated_list0, IResult,
};
use std::io::BufRead;

use crate::parse_string::{hashtag, link_or_word};

use super::LinesIterator;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HeaderParseState {
    None,
    YamlFrontMatter,
    AttrFrontMatter,
}

pub fn parse_page_header(
    lines: &mut LinesIterator<impl BufRead>,
) -> Result<FxHashMap<String, Vec<String>>, anyhow::Error> {
    let mut page_attrs = FxHashMap::default();
    let first_line = lines.next().transpose()?.unwrap_or_default();
    if first_line.is_empty() {
        return Ok(page_attrs);
    }

    let parse_attr_line = |separator, line: &str| {
        let parsed: Option<Result<_, anyhow::Error>> =
            line.split_once(separator)
                .map(|(attr_name, attr_value_str)| {
                    let attr_value_str = attr_value_str.trim();
                    let values = if attr_name == "tags" {
                        parse_tag_values(attr_value_str)?
                    } else {
                        vec![attr_value_str.to_string()]
                    };

                    Ok((attr_name.to_string(), values))
                });

        parsed
    };

    let header_state: HeaderParseState;
    if first_line.trim_end() == "---" {
        header_state = HeaderParseState::YamlFrontMatter;
    } else if !first_line.starts_with('-') {
        // Logseq Attribute front matter style

        // The first line is actually an attribute so we need to parse it.
        let parsed = parse_attr_line("::", first_line.as_str());

        match parsed {
            Some(Ok((attr_name, attr_values))) => {
                header_state = HeaderParseState::AttrFrontMatter;
                page_attrs.insert(attr_name, attr_values);
            }
            _ => {
                // It wasn't actually an attribute, so exit header parse mode.
                header_state = HeaderParseState::None;
            }
        };
    } else {
        header_state = HeaderParseState::None;
    }

    let putback_line = if header_state == HeaderParseState::None {
        first_line
    } else {
        loop {
            let line = match (header_state, lines.next()) {
                (_, None) => {
                    return Ok(page_attrs);
                }
                (_, Some(Err(e))) => return Err(e.into()),
                (HeaderParseState::None, _) => panic!("In header parse where state is None"),
                (HeaderParseState::AttrFrontMatter, Some(Ok(line))) => {
                    if line.starts_with('-') {
                        // This is the start of the real content, so return the line.
                        break line;
                    }
                    line
                }
                (HeaderParseState::YamlFrontMatter, Some(Ok(line))) => {
                    if line == "---" {
                        // This is the end of the header, but not real content, so just return an
                        // empty string.
                        break String::new();
                    }
                    line
                }
            };

            let separator = if header_state == HeaderParseState::YamlFrontMatter {
                ":"
            } else {
                "::"
            };

            let parsed = parse_attr_line(separator, line.as_str());

            match parsed {
                Some(Ok((attr_name, attr_values))) => page_attrs.insert(attr_name, attr_values),
                _ => break line,
            };
        }
    };

    if !putback_line.is_empty() {
        lines.put_back(Ok(putback_line));
    }

    Ok(page_attrs)
}

fn tag_value_separator(input: &str) -> IResult<&str, &str> {
    take_while1(|c| is_space(c as u8) || c == ',')(input)
}

fn parse_tag_value(input: &str) -> IResult<&str, &str> {
    alt((map(hashtag, |(value, _)| value), link_or_word))(input)
}

fn parse_tag_values(input: &str) -> Result<Vec<String>, anyhow::Error> {
    let values = match separated_list0(tag_value_separator, parse_tag_value)(input) {
        Ok((_, values)) => values,
        Err(e) => return Err(anyhow!("Parsing {}: {}", input, e)),
    };

    Ok(values.iter().map(|v| v.to_string()).collect::<Vec<_>>())
}

#[cfg(test)]
mod test {

    mod page_header {
        use std::{io::BufRead, iter::FromIterator};

        use fxhash::FxHashMap;
        use indoc::indoc;
        use itertools::put_back;

        use super::super::parse_page_header;

        fn run_test(
            input: &str,
        ) -> Result<(String, FxHashMap<String, Vec<String>>), anyhow::Error> {
            let mut reader = put_back(std::io::BufReader::new(input.as_bytes()).lines());
            let attrs = parse_page_header(&mut reader)?;

            let next_line = reader.next().transpose()?.unwrap_or_default();
            Ok((next_line, attrs))
        }

        #[test]
        fn no_frontmatter() {
            let input = r##"- the first block
                - another block
                "##;

            assert_eq!(
                run_test(input).unwrap(),
                (
                    String::from("- the first block"),
                    FxHashMap::<String, Vec<String>>::default()
                )
            );
        }

        #[test]
        fn empty_yaml_frontmatter() {
            let input = indoc! { r##"
                ---
                ---
                - the first block
                - another block
                "##
            };

            println!("{}", input);

            assert_eq!(
                run_test(input).unwrap(),
                (
                    String::from("- the first block"),
                    FxHashMap::<String, Vec<String>>::default()
                )
            );
        }

        #[test]
        fn yaml_frontmatter() {
            let input = indoc! { r##"
                ---
                title: It's a title
                tags: a, b, c
                ---
                - some text
                "##


            };

            assert_eq!(
                run_test(input).unwrap(),
                (
                    String::from("- some text"),
                    FxHashMap::<String, Vec<String>>::from_iter([
                        (String::from("title"), vec![String::from("It's a title")]),
                        (
                            String::from("tags"),
                            vec![String::from("a"), String::from("b"), String::from("c")]
                        )
                    ])
                )
            );
        }

        #[test]
        fn attr_frontmatter() {
            let input = indoc! { r##"
                title:: It's a title
                tags:: a, b, c
                - some text
                "##


            };

            assert_eq!(
                run_test(input).unwrap(),
                (
                    String::from("- some text"),
                    FxHashMap::<String, Vec<String>>::from_iter([
                        (String::from("title"), vec![String::from("It's a title")]),
                        (
                            String::from("tags"),
                            vec![String::from("a"), String::from("b"), String::from("c")]
                        )
                    ])
                )
            );
        }
    }

    mod tag_values {
        use super::super::{parse_tag_value, parse_tag_values, tag_value_separator};

        #[test]
        fn separator() {
            tag_value_separator(" ").expect("parsing space");
            tag_value_separator(",").expect("parsing comma");
            tag_value_separator(", ").expect("parsing comma with trailing space");
            tag_value_separator(" ,").expect("parsing comma with leading space");
            tag_value_separator(" , ").expect("parsing comman with spaces on both sides");
        }

        #[test]
        fn single_tag_values() {
            assert_eq!(parse_tag_value("#abc").expect("hashtag"), ("", "abc"));
            assert_eq!(parse_tag_value("abc").expect("raw value"), ("", "abc"));
            assert_eq!(
                parse_tag_value("[[abc def]]").expect("link"),
                ("", "abc def")
            );
        }

        #[test]
        fn one_hashtag() {
            assert_eq!(parse_tag_values("#abc").expect("parsing"), vec!["abc"])
        }

        #[test]
        fn two_hashtags() {
            assert_eq!(
                parse_tag_values("#abc #def").expect("parsing"),
                vec!["abc", "def"]
            )
        }

        #[test]
        fn two_raw_values() {
            assert_eq!(
                parse_tag_values("abc def").expect("parsing"),
                vec!["abc", "def"]
            )
        }

        #[test]
        fn hashtags_with_commas() {
            assert_eq!(
                parse_tag_values("#abc, #def").expect("parsing"),
                vec!["abc", "def"]
            )
        }

        #[test]
        fn values_with_commas() {
            assert_eq!(
                parse_tag_values("abc, def").expect("parsing"),
                vec!["abc", "def"]
            )
        }
    }
}
