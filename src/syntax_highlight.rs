use syntect::html;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub struct Highlighter {
    syntax_set: SyntaxSet,
    class_style: html::ClassStyle,
}

impl Highlighter {
    pub fn new(class_prefix: Option<&'static str>) -> Highlighter {
        let mut ss = SyntaxSet::load_defaults_newlines().into_builder();
        ss.add_plain_text_syntax();

        let class_style = class_prefix
            .map(|p| html::ClassStyle::SpacedPrefixed { prefix: p })
            .unwrap_or(html::ClassStyle::Spaced);

        Highlighter {
            syntax_set: ss.build(),
            class_style,
        }
    }

    pub fn highlight(&self, text: &str) -> Result<String, anyhow::Error> {
        let mut lines = LinesWithEndings::from(text);

        let first_line = lines.next().unwrap_or("").trim();
        let syntax = self
            .syntax_set
            .find_syntax_by_token(first_line)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut highlighter = html::ClassedHTMLGenerator::new_with_class_style(
            syntax,
            &self.syntax_set,
            self.class_style,
        );

        for line in lines {
            highlighter.parse_html_for_line_which_includes_newline(line)?;
        }

        Ok(highlighter.finalize())
    }
}
