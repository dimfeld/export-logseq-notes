use eyre::{eyre, Result};
use nom::{
    branch::alt, bytes::complete::take_while1, combinator::map, multi::separated_list0, IResult,
};
use smallvec::{smallvec, SmallVec};

use crate::graph::AttrList;
use crate::parse_string::{hashtag, link_or_word};

pub fn parse_attr_line(separator: &str, line: &str) -> Result<Option<(String, AttrList)>> {
    line.split_once(separator)
        .filter(|(attr_name, _)| !attr_name.chars().any(|c| c.is_whitespace()))
        .map(|(attr_name, attr_value_str)| {
            let attr_value_str = attr_value_str.trim();
            let values = if attr_name.to_lowercase() == "tags" {
                parse_tag_values(attr_value_str)?
            } else {
                smallvec![attr_value_str.to_string()]
            };

            Ok((attr_name.to_string(), values))
        })
        .transpose()
}

fn tag_value_separator(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c.is_whitespace() || c == ',')(input)
}

fn parse_tag_value(input: &str) -> IResult<&str, &str> {
    alt((map(hashtag, |(value, _)| value), link_or_word))(input)
}

fn parse_tag_values(input: &str) -> Result<AttrList> {
    let values = match separated_list0(tag_value_separator, parse_tag_value)(input) {
        Ok((_, values)) => values,
        Err(e) => return Err(eyre!("Parsing {}: {}", input, e)),
    };

    Ok(values
        .iter()
        .map(|v| v.to_string())
        .collect::<SmallVec<_>>())
}

#[cfg(test)]
mod tests {
    use super::{parse_tag_value, parse_tag_values, tag_value_separator};
    use crate::graph::AttrList;

    fn test_attr_values(values: &[&'static str]) -> AttrList {
        values.iter().map(|s| s.to_string()).collect()
    }

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
        assert_eq!(
            parse_tag_values("#abc").expect("parsing"),
            test_attr_values(&["abc"])
        )
    }

    #[test]
    fn two_hashtags() {
        assert_eq!(
            parse_tag_values("#abc #def").expect("parsing"),
            test_attr_values(&["abc", "def"])
        )
    }

    #[test]
    fn two_raw_values() {
        assert_eq!(
            parse_tag_values("abc def").expect("parsing"),
            test_attr_values(&["abc", "def"])
        )
    }

    #[test]
    fn hashtags_with_commas() {
        assert_eq!(
            parse_tag_values("#abc, #def").expect("parsing"),
            test_attr_values(&["abc", "def"])
        )
    }

    #[test]
    fn values_with_commas() {
        assert_eq!(
            parse_tag_values("abc, def").expect("parsing"),
            test_attr_values(&["abc", "def"])
        )
    }
}
