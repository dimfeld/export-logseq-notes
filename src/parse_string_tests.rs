use crate::parse_string::{Expression::*, *};

fn test_parse_all_styles(input: &str, expected: Vec<Expression>) {
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        expected,
        "roam style"
    );
    assert_eq!(
        parse(ContentStyle::Logseq, input).unwrap(),
        expected,
        "logseq style"
    );
}

#[test]
fn word() {
    let input = "word";
    test_parse_all_styles(input, vec![Expression::Text("word")]);
}

#[test]
fn words() {
    let input = "two words";
    test_parse_all_styles(input, vec![Expression::Text("two words")]);
}

#[test]
fn surrounding_whitespace() {
    let input = "  two words  ";
    test_parse_all_styles(input, vec![Expression::Text("  two words  ")])
}

#[test]
fn block_ref() {
    let input = "((a ref))";
    test_parse_all_styles(input, vec![Expression::BlockRef("a ref")])
}

#[test]
fn link() {
    let input = "[[a title]]";
    test_parse_all_styles(input, vec![Expression::Link("a title")])
}

#[test]
fn hashtag_simple() {
    let input = "#tag";
    test_parse_all_styles(input, vec![Hashtag("tag", false)])
}

#[test]
fn hashtag_with_link() {
    let input = "#[[a tag]]";
    test_parse_all_styles(input, vec![Expression::Hashtag("a tag", false)])
}

#[test]
fn hashtag_with_dot() {
    let input = "#.tag";
    test_parse_all_styles(input, vec![Expression::Hashtag("tag", true)])
}

#[test]
fn other_brace() {
    let input = "{{ something-else }}";
    test_parse_all_styles(input, vec![Expression::BraceDirective("something-else")])
}

#[test]
fn table_brace() {
    let input = "{{ table }}";
    test_parse_all_styles(input, vec![Table]);
}

#[test]
fn hashtag_brace() {
    // This isn't valid in Roam, so it doesn't parse out the hashtag.
    let input = "{{ #table}}";
    test_parse_all_styles(input, vec![Expression::BraceDirective("#table")])
}

#[test]
fn link_with_enclosed_bracket() {
    let input = "[[ab[cd]ef]]";
    test_parse_all_styles(input, vec![Expression::Link("ab[cd]ef")])
}

#[test]
fn table_link_brace() {
    let input = "{{[[table]]}}";
    test_parse_all_styles(input, vec![Table])
}

#[test]
fn other_link_brace() {
    let input = "{{[[something-else]]}}";
    test_parse_all_styles(input, vec![BraceDirective("something-else")])
}

#[test]
fn multiword_with_links() {
    let input = "I want an [[astrolabe]] of my own";
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("I want an "),
            Expression::Link("astrolabe"),
            Expression::Text(" of my own"),
        ],
    )
}

#[test]
fn single_brace() {
    let input = "this is not [a brace ] but [[this is]]";
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("this is not [a brace ] but "),
            Expression::Link("this is"),
        ],
    )
}

#[test]
fn single_bracket() {
    let input = "this is not {a bracket } but [[this is a]]link";
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("this is not {a bracket } but "),
            Expression::Link("this is a"),
            Expression::Text("link"),
        ],
    )
}

#[test]
fn roam_fake_bold() {
    let input = "this is *not* bold or italic";
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![Text("this is *not* bold or italic")]
    );
}

#[test]
fn image() {
    let input =
    "![](https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc)";
    test_parse_all_styles(
        input,
        vec![Expression::Image {
      alt: "",
      url: "https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc"
    }],
    )
}

#[test]
fn image_with_alt() {
    let input =
    "![some alt text](https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc)";
    test_parse_all_styles(
        input,
        vec![Expression::Image {
      alt: "some alt text",
      url: "https://firebasestorage.googleapis.com/v0/b/firescript-577a2.appspot.com/o/some-id?abc"
    }],
    )
}

#[test]
fn real_world_1() {
    let input = r##"An initially \"honest\" signal becomes dishonest."##;
    test_parse_all_styles(
        input,
        vec![Expression::Text(
            r##"An initially \"honest\" signal becomes dishonest."##,
        )],
    )
}

#[test]
fn plaintext_link() {
    let input = r##"Source: https://a.website.com/is-post"##;
    test_parse_all_styles(
        input,
        vec![
            Expression::Text(r##"Source: "##),
            RawHyperlink("https://a.website.com/is-post"),
        ],
    )
}

#[test]
fn plaintext_link_entire_string() {
    let input = "https://www.example.com/def/ghi?abc=def#an-anchor";
    test_parse_all_styles(input, vec![Expression::RawHyperlink(input)]);
}

#[test]
fn plaintext_link_omits_trailing_character() {
    let input = "at https://www.example.com/def.";
    test_parse_all_styles(
        input,
        vec![
            Text("at "),
            RawHyperlink("https://www.example.com/def"),
            Text("."),
        ],
    );
}

#[test]
fn plaintext_link_omits_trailing_character2() {
    let input = "at https://www.example.com/def/ghi?abc=def#an-anchor.";
    test_parse_all_styles(
        input,
        vec![
            Text("at "),
            RawHyperlink("https://www.example.com/def/ghi?abc=def#an-anchor"),
            Text("."),
        ],
    );
}

#[test]
fn markdown_link() {
    let input =
        r##"For actually communicating, [spiped](https://www.tarsnap.com/spiped.html) is nice"##;
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("For actually communicating, "),
            Expression::MarkdownExternalLink {
                title: "spiped",
                url: "https://www.tarsnap.com/spiped.html",
            },
            Expression::Text(" is nice"),
        ],
    )
}

#[test]
fn markdown_link_with_embedded_parens() {
    let input =
        r##"For actually communicating, [spiped](https://www.tarsnap.com/sp(i)ped.html) is nice"##;
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("For actually communicating, "),
            Expression::MarkdownExternalLink {
                title: "spiped",
                url: "https://www.tarsnap.com/sp(i)ped.html",
            },
            Expression::Text(" is nice"),
        ],
    )
}

#[test]
fn attribute_simple() {
    let input = "Source:: some blog";
    test_parse_all_styles(
        input,
        vec![Expression::Attribute {
            name: "Source",
            value: vec![Expression::Text("some blog")],
        }],
    )
}

#[test]
fn attribute_nospace() {
    let input = "Source::some blog";
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![Expression::Attribute {
            name: "Source",
            value: vec![Expression::Text("some blog")],
        }],
    )
}

#[test]
fn roam_attribute_complex() {
    let input = " My Score:: too [[high]] to count";
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![Expression::Attribute {
            name: " My Score",
            value: vec![
                Expression::Text("too "),
                Expression::Link("high"),
                Expression::Text(" to count"),
            ],
        }],
    )
}

#[test]
fn roam_attribute_extra_colons() {
    let input = " My Score::: too :: high :: to count";
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![Expression::Attribute {
            name: " My Score",
            value: vec![Expression::Text(": too :: high :: to count")],
        }],
    )
}

#[test]
fn isolated_attribute() {
    let input = "completed:: true";
    assert_eq!(
        attribute(ContentStyle::Logseq, input).unwrap(),
        ("", ("completed", vec![Expression::Text("true")],)),
    )
}

#[test]
fn logseq_attribute_and_text_in_block() {
    let input = "Some text\n   completed:: true";
    assert_eq!(
        parse(ContentStyle::Logseq, input).unwrap(),
        vec![
            Expression::Text("Some text"),
            Expression::Attribute {
                name: "completed",
                value: vec![Expression::Text("true")],
            },
        ],
    )
}

#[test]
fn logseq_colons_in_attribute_value() {
    let input = "completed:: true:: false";
    assert_eq!(
        attribute(ContentStyle::Logseq, input).unwrap(),
        ("", ("completed", vec![Expression::Text("true:: false")],)),
    )
}

#[test]
fn roam_attribute_backticks_1() {
    // Do not parse it as an attribute if the :: is inside backticks
    let input = " My Score ` :: too [[high]] to count`";
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![
            Expression::Text(" My Score "),
            Expression::SingleBacktick(" :: too [[high]] to count"),
        ],
    )
}

#[test]
fn roam_attribute_backticks_2() {
    // This feels weird but it matches Roam's behavior.
    // Understandable since it's difficult to parse otherwise
    let input = "My `Score`:: too [[high]] to count";
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![
            Expression::Text("My "),
            Expression::SingleBacktick("Score"),
            Expression::Text(":: too "),
            Expression::Link("high"),
            Expression::Text(" to count"),
        ],
    )
}

#[test]
fn exclamation_point() {
    let input = "This is exciting!";
    test_parse_all_styles(input, vec![Text("This is exciting!")]);
}

#[test]
fn real_world_2() {
    let input = "Added support for switchable transition styles to [[svelte-zoomable]]";
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("Added support for switchable transition styles to "),
            Expression::Link("svelte-zoomable"),
        ],
    )
}

#[test]
fn real_world_3() {
    let input = "Include `hostnames;` inside the block to let it do wildcard matches on hostnames.";
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("Include "),
            Expression::SingleBacktick("hostnames;"),
            Expression::Text(" inside the block to let it do wildcard matches on hostnames."),
        ],
    )
}

#[test]
fn real_world_4() {
    let input = r##"**Algorithm - Difference Engine** #roam/templates"##;
    test_parse_all_styles(
        input,
        vec![
            Bold(vec![Text("Algorithm - Difference Engine")]),
            Text(" "),
            Hashtag("roam/templates", false),
        ],
    )
}

#[test]
fn real_world_5() {
    let input = r##"{{[[TODO]]}} [[Projects/Rewrite everything]]"##;
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![
            Expression::Todo { done: false },
            Expression::Text(" "),
            Expression::Link("Projects/Rewrite everything"),
        ],
    )
}

#[test]
fn real_world_6() {
    let input = r##"{{[[TODO]]}}[[Projects/Rewrite everything]]"##;
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![
            Expression::Todo { done: false },
            Expression::Link("Projects/Rewrite everything"),
        ],
    );
}

#[test]
fn real_world_7() {
    let input =
        r##"([Location 1062](https://readwise.io/to_kindle?action=open&asin=2232&location=1062))"##;
    test_parse_all_styles(
        input,
        vec![
            Expression::Text("("),
            Expression::MarkdownExternalLink {
                title: "Location 1062",
                url: "https://readwise.io/to_kindle?action=open&asin=2232&location=1062",
            },
            Expression::Text(")"),
        ],
    )
}

#[test]
fn real_world_8() {
    let input = r##"--- **John 13:18-30 - Judas and Jesus** ---"##;
    test_parse_all_styles(
        input,
        vec![
            Text("--- "),
            Bold(vec![Text("John 13:18-30 - Judas and Jesus")]),
            Text(" ---"),
        ],
    )
}

#[test]
fn triple_backtick_1() {
    let input = r##"```javascript\nmap $regex_domain $domain {\n  app defaultskin;\n  tm defaultskin;\n  www defaultskin;\n  '' defaultskin;\n  dev defaultskin;\n  default $regex_domain;\n}```"##;
    test_parse_all_styles(
        input,
        vec![Expression::TripleBacktick(
            r##"javascript\nmap $regex_domain $domain {\n  app defaultskin;\n  tm defaultskin;\n  www defaultskin;\n  '' defaultskin;\n  dev defaultskin;\n  default $regex_domain;\n}"##,
        )],
    )
}

#[test]
fn triple_backtick_2() {
    let input = r##"```css\nbackground: #203;\ncolor: #ffc;\ntext-shadow: 0 0 .1em, 0 0 .3em;```"##;
    test_parse_all_styles(
        input,
        vec![Expression::TripleBacktick(
            r##"css\nbackground: #203;\ncolor: #ffc;\ntext-shadow: 0 0 .1em, 0 0 .3em;"##,
        )],
    )
}

#[test]
fn roam_todo() {
    let input = r##"{{[[TODO]]}} Get things done"##;
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![
            Expression::Todo { done: false },
            Expression::Text(" Get things done"),
        ],
        "roam style works"
    );

    assert_eq!(
        parse(ContentStyle::Logseq, input).unwrap(),
        vec![
            Expression::BraceDirective("TODO"),
            Expression::Text(" Get things done"),
        ],
        "logseq doesn't parse as todo"
    );
}

#[test]
fn logseq_todo() {
    let input = r##"TODO Get things done"##;
    assert_eq!(
        parse(ContentStyle::Logseq, input).unwrap(),
        vec![
            Expression::Todo { done: false },
            Expression::Text(" Get things done"),
        ],
    );
}

#[test]
fn logseq_done() {
    let input = r##"DONE Get things done"##;
    assert_eq!(
        parse(ContentStyle::Logseq, input).unwrap(),
        vec![
            Expression::Todo { done: true },
            Expression::Text(" Get things done"),
        ],
    );
}

#[test]
fn logseq_todo_must_be_at_start() {
    let input = r##" TODO Get things done"##;
    assert_eq!(
        parse(ContentStyle::Roam, input).unwrap(),
        vec![Expression::Text(" TODO Get things done"),],
    );
}

#[test]
fn unicode() {
    let input = r##"client’s merkle tree"##;
    test_parse_all_styles(input, vec![Expression::Text("client’s merkle tree")])
}

#[test]
fn blockquote_simple() {
    let input = r##"> Some text"##;
    test_parse_all_styles(
        input,
        vec![Expression::BlockQuote(vec![Expression::Text("Some text")])],
    );
}

#[test]
fn blockquote_with_nested_styles() {
    let input = r##"> [[Some]] **text**"##;
    test_parse_all_styles(
        input,
        vec![Expression::BlockQuote(vec![
            Expression::Link("Some"),
            Expression::Text(" "),
            Expression::Bold(vec![Expression::Text("text")]),
        ])],
    );
}

#[test]
fn blockquote_fake_1() {
    let input = r##" > Some text"##;
    test_parse_all_styles(input, vec![Expression::Text(" > Some text")]);
}

#[test]
fn blockquote_fake_2() {
    let input = r##"Some text
> and another"##;
    test_parse_all_styles(input, vec![Expression::Text("Some text\n> and another")]);
}
