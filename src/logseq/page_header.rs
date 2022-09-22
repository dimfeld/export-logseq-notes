use eyre::Result;
use std::io::BufRead;

use crate::graph::AttrList;

use super::LinesIterator;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HeaderParseState {
    None,
    YamlFrontMatter,
    AttrFrontMatter,
}

pub fn parse_page_header(
    lines: &mut LinesIterator<impl BufRead>,
) -> Result<Vec<(String, AttrList)>> {
    let mut page_attrs = Vec::new();
    let first_line = lines.next().transpose()?.unwrap_or_default();
    if first_line.is_empty() {
        return Ok(page_attrs);
    }

    let header_state: HeaderParseState;
    if first_line.trim_end() == "---" {
        header_state = HeaderParseState::YamlFrontMatter;
    } else if !first_line.starts_with('-') {
        // Logseq Attribute front matter style

        // The first line is actually an attribute so we need to parse it.
        let parsed = super::attrs::parse_attr_line("::", first_line.as_str());

        match parsed {
            Ok(Some((attr_name, attr_values))) => {
                header_state = HeaderParseState::AttrFrontMatter;
                page_attrs.push((attr_name, attr_values));
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

            let parsed = super::attrs::parse_attr_line(separator, line.as_str());

            match parsed {
                Ok(Some((attr_name, attr_values))) => page_attrs.push((attr_name, attr_values)),
                _ => break line,
            };
        }
    };

    if !putback_line.is_empty() {
        lines.put_back(Ok(putback_line));
    }

    Ok(page_attrs)
}

#[cfg(test)]
mod test {

    use std::io::BufRead;

    use eyre::Result;
    use indoc::indoc;
    use itertools::put_back;
    use smallvec::smallvec;

    use crate::graph::AttrList;

    use super::parse_page_header;

    fn run_test(input: &str) -> Result<(String, Vec<(String, AttrList)>)> {
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
            (String::from("- the first block"), Vec::new())
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
            (String::from("- the first block"), Vec::new())
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
                vec![
                    (
                        String::from("title"),
                        smallvec![String::from("It's a title")]
                    ),
                    (
                        String::from("tags"),
                        smallvec![String::from("a"), String::from("b"), String::from("c")]
                    )
                ]
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
                vec![
                    (
                        String::from("title"),
                        smallvec![String::from("It's a title")]
                    ),
                    (
                        String::from("tags"),
                        smallvec![String::from("a"), String::from("b"), String::from("c")]
                    )
                ]
            )
        );
    }
}
