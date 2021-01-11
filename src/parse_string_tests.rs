#[cfg(test)]
use crate::parse_string::*;

#[test]
fn word() {
  let input = "word";
  assert_eq!(parse(input).unwrap(), vec![Expression::Text("word")])
}

#[test]
fn words() {
  let input = "two words";
  assert_eq!(parse(input).unwrap(), vec![Expression::Text("two words")])
}

#[test]
fn surrounding_whitespace() {
  let input = "  two words  ";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Text("  two words  ")]
  )
}

#[test]
fn block_ref() {
  let input = "((a ref))";
  assert_eq!(parse(input).unwrap(), vec![Expression::BlockRef("a ref")])
}

#[test]
fn link() {
  let input = "[[a title]]";
  assert_eq!(parse(input).unwrap(), vec![Expression::Link("a title")])
}

#[test]
fn hashtag_simple() {
  let input = "#tag";
  assert_eq!(parse(input).unwrap(), vec![Expression::Hashtag("tag")])
}

#[test]
fn hashtag_with_link() {
  let input = "#[[a tag]]";
  assert_eq!(parse(input).unwrap(), vec![Expression::Hashtag("a tag")])
}

#[test]
fn simple_brace() {
  let input = "{{ table }}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("table")]
  )
}

#[test]
fn hashtag_brace() {
  // This isn't valid in Roam, so it doesn't parse out the hashtag.
  let input = "{{ #table}}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("#table")]
  )
}

#[test]
fn link_with_enclosed_bracket() {
  let input = "[[ab[cd]ef]]";
  assert_eq!(parse(input).unwrap(), vec![Expression::Link("ab[cd]ef")])
}

#[test]
fn link_brace() {
  let input = "{{[[table]]}}";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::BraceDirective("table")]
  )
}

#[test]
fn multiword_with_links() {
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
fn single_brace() {
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
fn single_bracket() {
  let input = "this is not {a bracket } but [[this is a]]link";
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("this is not "),
      Expression::Text("{a bracket } but "),
      Expression::Link("this is a"),
      Expression::Text("link")
    ]
  )
}

#[test]
fn image() {
  let input =
    "![](https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc)";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Image {
      alt: "",
      url: "https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc"
    }]
  )
}

#[test]
fn image_with_alt() {
  let input =
    "![some alt text](https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc)";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Image {
      alt: "some alt text",
      url: "https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc"
    }]
  )
}

#[test]
fn real_world_1() {
  let input = r##"An initially \"honest\" signal becomes dishonest."##;
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Text(
      r##"An initially \"honest\" signal becomes dishonest."##
    )]
  )
}

#[test]
fn plaintext_link() {
  let input = r##"Source: https://a.website.com/is-post"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Text(
      r##"Source: https://a.website.com/is-post"##
    )]
  )
}

#[test]
fn markdown_link() {
  let input =
    r##"For actually communicating, [spiped](https://www.tarsnap.com/spiped.html) is nice"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("For actually communicating, "),
      Expression::MarkdownLink {
        title: "spiped",
        url: "https://www.tarsnap.com/spiped.html"
      },
      Expression::Text(" is nice")
    ]
  )
}

#[test]
fn attribute_simple() {
  let input = "Source:: some blog";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Attribute {
      name: "Source",
      value: vec![Expression::Text("some blog")]
    }]
  )
}

#[test]
fn attribute_nospace() {
  let input = "Source::some blog";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Attribute {
      name: "Source",
      value: vec![Expression::Text("some blog")]
    }]
  )
}

#[test]
fn attribute_complex() {
  let input = " My Score:: too [[high]] to count";
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::Attribute {
      name: " My Score",
      value: vec![
        Expression::Text("too "),
        Expression::Link("high"),
        Expression::Text(" to count")
      ]
    }]
  )
}

#[test]
fn real_world_2() {
  let input = "Added support for switchable transition styles to [[svelte-zoomable]]";
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("Added support for switchable transition styles to "),
      Expression::Link("svelte-zoomable")
    ]
  )
}

#[test]
fn real_world_3() {
  let input = "Include `hostnames;` inside the block to let it do wildcard matches on hostnames.";
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("Include "),
      Expression::SingleBacktick("hostnames;"),
      Expression::Text(" inside the block to let it do wildcard matches on hostnames.")
    ]
  )
}

#[test]
fn real_world_4() {
  let input = r##"**Algorithm - Difference Engine** #roam/templates"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("**Algorithm - Difference Engine** "),
      Expression::Hashtag("roam/templates"),
    ]
  )
}

#[test]
fn real_world_5() {
  let input = r##"{{[[TODO]]}} [[Projects/Rewrite everything]]"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::BraceDirective("TODO"),
      Expression::Text(" "),
      Expression::Link("Projects/Rewrite everything"),
    ]
  )
}

#[test]
fn real_world_6() {
  let input = r##"{{[[TODO]]}}[[Projects/Rewrite everything]]"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::BraceDirective("TODO"),
      Expression::Link("Projects/Rewrite everything"),
    ]
  )
}

#[test]
fn real_world_7() {
  let input =
    r##"([Location 1062](https://readwise.io/to_kindle?action=open&asin=2232&location=1062))"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::Text("("),
      Expression::MarkdownLink {
        title: "Location 1062",
        url: "https://readwise.io/to_kindle?action=open&asin=2232&location=1062"
      },
      Expression::Text(")"),
    ]
  )
}

#[test]
fn triple_backtick_1() {
  let input = r##"```javascript\nmap $regex_domain $domain {\n  app defaultskin;\n  tm defaultskin;\n  www defaultskin;\n  '' defaultskin;\n  dev defaultskin;\n  default $regex_domain;\n}```"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::TripleBacktick(
      r##"javascript\nmap $regex_domain $domain {\n  app defaultskin;\n  tm defaultskin;\n  www defaultskin;\n  '' defaultskin;\n  dev defaultskin;\n  default $regex_domain;\n}"##
    )]
  )
}

#[test]
fn triple_backtick_2() {
  let input = r##"```css\nbackground: #203;\ncolor: #ffc;\ntext-shadow: 0 0 .1em, 0 0 .3em;```"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![Expression::TripleBacktick(
      r##"css\nbackground: #203;\ncolor: #ffc;\ntext-shadow: 0 0 .1em, 0 0 .3em;"##
    )]
  )
}

#[test]
fn todo() {
  let input = r##"{{[[TODO]]}} Get things done"##;
  assert_eq!(
    parse(input).unwrap(),
    vec![
      Expression::BraceDirective("TODO"),
      Expression::Text(" Get things done")
    ]
  )
}
