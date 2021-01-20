use crate::html;
use crate::parse_string::{parse, Expression};
use crate::roam_edn::*;
use crate::string_builder::StringBuilder;
use crate::syntax_highlight;
use anyhow::{anyhow, Result};
use fxhash::FxHashMap;
use serde::Serialize;

pub struct TitleSlugUid {
    pub title: String,
    pub slug: String,
    pub uid: String,
}

pub struct IdSlugUid {
    pub id: usize,
    pub slug: String,
    pub uid: String,
}

#[derive(Serialize)]
pub struct TitleAndUid {
    pub title: String,
    pub uid: String,
}

pub struct Page<'a, 'b> {
    pub id: usize,
    pub title: String,

    pub filter_tag: &'a str,
    pub graph: &'a Graph,
    pub omit_blocks_with_only_unexported_links: bool,
    pub included_pages_by_title: &'a FxHashMap<String, IdSlugUid>,
    pub included_pages_by_id: &'a FxHashMap<usize, TitleSlugUid>,
    pub highlighter: &'b syntax_highlight::Highlighter,
}

fn write_depth(depth: usize) -> String {
    "  ".repeat(depth)
}

impl<'a, 'b> Page<'a, 'b> {
    fn link_if_allowed(&self, s: &'a str) -> StringBuilder<'a> {
        self.included_pages_by_title
            .get(s)
            .map(|IdSlugUid { slug, .. }| {
                StringBuilder::from(format!(
                    r##"<a href="{slug}">{title}</a>"##,
                    title = html::escape(s),
                    slug = html::escape(slug)
                ))
            })
            .unwrap_or_else(|| StringBuilder::from(html::escape(s)))
    }

    fn render_block_ref(
        &self,
        containing_block: &'a Block,
        s: &'a str,
    ) -> Result<(StringBuilder<'a>, bool)> {
        let block = self.graph.block_from_uid(s);
        match block {
            Some(block) => self
                .render_line_without_header(block)
                .map(|(result, _, _)| {
                    match self.included_pages_by_id.get(&block.page) {
                        Some(page) => {
                            // When the referenced page is exported, make this a link to the block.
                            let linked = StringBuilder::Vec(vec![
                                StringBuilder::from(format!(
                                    r##"<a class="block-ref" href="{page}#{block}">"##,
                                    page = page.slug,
                                    block = block.uid
                                )),
                                result,
                                StringBuilder::from("</a>"),
                            ]);

                            (linked, true)
                        }
                        None => (result, true),
                    }
                }),
            None => {
                // Block ref syntax can also be expandable text. So if we don't match on a block then just render it.
                parse(s)
                    .map_err(|e| anyhow!("Parse Error: {}", e))
                    .and_then(|expressions| self.render_expressions(containing_block, expressions))
            }
        }
    }

    fn hashtag(&self, s: &'a str, dot: bool) -> StringBuilder<'a> {
        if s == self.filter_tag {
            // Don't render the primary export tag
            return StringBuilder::Empty;
        }

        let anchor = self.link_if_allowed(s);
        if dot {
            StringBuilder::Vec(vec![
                StringBuilder::from(format!("<span class=\"{}\">", s)),
                anchor,
                StringBuilder::from("</span>"),
            ])
        } else {
            anchor
        }
    }

    fn render_block_embed(&self, s: &str) -> Result<StringBuilder<'a>> {
        self
      .graph
      .block_from_uid(s)
      .map(|block| self.render_block_and_children(block.id, 0).map(|rendered| {
        StringBuilder::from(vec![
          StringBuilder::from("<div class=\"roam-block-container rm-block rm-block--open rm-not-focused block-bullet-view\">"),
          rendered,
          StringBuilder::from("</div>")
        ])
      }))
      .unwrap_or(Ok(StringBuilder::Empty))
    }

    fn descend_table_child(
        &self,
        row: Vec<StringBuilder<'a>>,
        id: usize,
    ) -> Vec<Vec<StringBuilder<'a>>> {
        self.graph
            .blocks
            .get(&id)
            .map(|block| {
                let rendered = self.render_line_without_header(block).unwrap();
                let mut row = row.clone();
                row.push(rendered.0);
                block
                    .children
                    .iter()
                    .flat_map(|&child| self.descend_table_child(row.clone(), child))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![row])
    }

    /// Given a block containing a table, render that table into markdown format
    fn render_table(&self, block: &'a Block) -> StringBuilder<'a> {
        let rows = block
            .children
            .iter()
            .map(|id| self.descend_table_child(Vec::new(), *id))
            .map(|row| {
                let mut output = StringBuilder::with_capacity(row.len() * 3 + 2);
                output.push("<tr>\n");
                for cell in row {
                    output.push("<td>");
                    output.push(cell);
                    output.push("</td>");
                }
                output.push("</tr>");
                output
            })
            .collect::<Vec<StringBuilder>>();

        StringBuilder::Vec(vec![
            StringBuilder::from("<table>\n"),
            StringBuilder::from(rows),
            StringBuilder::from("</table>"),
        ])
    }

    fn render_brace_directive(&self, block: &'a Block, s: &'a str) -> (StringBuilder<'a>, bool) {
        let (value, render_children) = match s {
            "table" => (self.render_table(block), false),
            _ => (
                StringBuilder::from(format!("<pre>{}</pre>", html::escape(s))),
                true,
            ),
        };

        (value, render_children)
    }

    fn render_style(
        &self,
        block: &'a Block,
        tag: &'a str,
        class: &'a str,
        e: Vec<Expression<'a>>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        self.render_expressions(block, e).map(|(s, rc)| {
            (
                StringBuilder::from(vec![
                    StringBuilder::from(format!(
                        r##"<{tag} class="{class}">"##,
                        tag = tag,
                        class = class
                    )),
                    s,
                    StringBuilder::from(format!("</{}>", tag)),
                ]),
                rc,
            )
        })
    }

    fn render_expressions(
        &self,
        block: &'a Block,
        e: Vec<Expression<'a>>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        let num_exprs = e.len();
        e.into_iter()
            .map(|e| self.render_expression(block, e))
            .fold(
                Ok((StringBuilder::with_capacity(num_exprs), true)),
                |acc, r| {
                    acc.and_then(|(mut line, render_children)| {
                        r.map(|r| {
                            line.push(r.0);
                            (line, render_children && r.1)
                        })
                    })
                },
            )
    }

    fn render_attribute(
        &self,
        block: &'a Block,
        name: &'a str,
        contents: Vec<Expression<'a>>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        if name == self.filter_tag {
            return Ok((StringBuilder::Empty, true));
        }

        self.render_expressions(block, contents).map(|(s, rc)| {
            let mut output = StringBuilder::with_capacity(5);
            output.push(r##"<span><strong class="rm-attr-ref">"##);
            output.push(html::escape(name));
            output.push(":</strong>");
            output.push(s);
            output.push("</span>");

            (output, rc)
        })
    }

    fn render_expression(
        &self,
        block: &'a Block,
        e: Expression<'a>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        let (rendered, render_children) = match e {
            Expression::Hashtag(s, dot) => (self.hashtag(s, dot), true),
            Expression::Image { alt, url } => (
                format!(
                    r##"<img title="{alt}" src="{url}" />"##,
                    alt = html::escape(alt),
                    url = html::escape(url)
                )
                .into(),
                true,
            ),
            Expression::Link(s) => (self.link_if_allowed(s), true),
            Expression::MarkdownLink { title, url } => (
                format!(
                    r##"<a href="{url}">{title}</a>"##,
                    title = html::escape(title),
                    url = html::escape(url),
                )
                .into(),
                true,
            ),
            Expression::SingleBacktick(s) => {
                (format!("<code>{}</code>", html::escape(s)).into(), true)
            }
            Expression::TripleBacktick(s) => (
                format!("<pre><code>{}</code></pre>", self.highlighter.highlight(s)).into(),
                true,
            ),
            Expression::Bold(e) => self.render_style(block, "strong", "rm-bold", e)?,
            Expression::Italic(e) => self.render_style(block, "em", "rm-italics", e)?,
            Expression::Strike(e) => self.render_style(block, "del", "rm-strikethrough", e)?,
            Expression::Highlight(e) => self.render_style(block, "span", "rm-highlight", e)?,
            Expression::Text(s) => (html::escape(s).into(), true),
            Expression::BlockRef(s) => self.render_block_ref(block, s)?,
            Expression::BraceDirective(s) => self.render_brace_directive(block, s),
            Expression::Table => (self.render_table(block), false),
            Expression::HRule => (r##"<hr class="rm-hr" />"##.into(), true),
            Expression::BlockEmbed(s) => (self.render_block_embed(s)?, true),
            Expression::PageEmbed(s) => (
                self.included_pages_by_title
                    .get(s)
                    .map(|IdSlugUid { id: block_id, .. }| {
                        self.render_block_and_children(*block_id, 0)
                            .map(|embedded_page| {
                                StringBuilder::from(vec![
                                    StringBuilder::from(format!(
                                        r##"<div class="rm-embed rm-embed--page rm-embed-container">
                  <h3 class="rm-page__title">{}</h3>
                  <div class="rm-embed__content">"##,
                                        s
                                    )),
                                    embedded_page,
                                    StringBuilder::from("</div>\n</div>"),
                                ])
                            })
                    })
                    .unwrap_or(Ok(StringBuilder::Empty))?,
                true,
            ),
            Expression::Attribute { name, value } => self.render_attribute(block, name, value)?, // TODO
        };

        Ok((rendered, render_children))
    }

    fn render_line_without_header(
        &self,
        block: &'a Block,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        let parsed = parse(&block.string).map_err(|e| anyhow!("Parse Error: {:?}", e))?;

        let single_unexported_link = match parsed.as_slice() {
            &[Expression::Link(title)] => self.included_pages_by_title.get(title).is_some(),
            &[Expression::Hashtag(title, _)] => self.included_pages_by_title.get(title).is_some(),
            _ => false,
        };

        self.render_expressions(block, parsed)
            .map(|(strings, render_children)| (strings, render_children, single_unexported_link))
    }

    fn render_line(&self, block: &'a Block) -> Result<(StringBuilder<'a>, bool, bool)> {
        self.render_line_without_header(block).map(|result| {
            if block.heading > 0 {
                (
                    StringBuilder::Vec(vec![
                        StringBuilder::from(format!(
                            "<div class=\"rm-heading-{}\">",
                            block.heading
                        )),
                        result.0,
                        StringBuilder::from("</div>"),
                    ]),
                    result.1,
                    result.2,
                )
            } else {
                result
            }
        })
    }

    fn render_block_and_children(
        &self,
        block_id: usize,
        depth: usize,
    ) -> Result<StringBuilder<'a>> {
        let block = self.graph.blocks.get(&block_id).unwrap();

        let (rendered, render_children, single_unexported_link) = self.render_line(block)?;
        let render_children = render_children && !block.children.is_empty();

        let render_line = !single_unexported_link || !self.omit_blocks_with_only_unexported_links;

        if (rendered.is_empty() || !render_line) && !render_children {
            return Ok(StringBuilder::Empty);
        }

        let render_li = depth > 0;

        let mut result = StringBuilder::with_capacity(9);
        result.push(write_depth(depth));

        if render_li {
            result.push(format!(r##"<li id="{id}">"##, id = block.uid));
        }

        if render_line {
            result.push(rendered);
        }

        // println!(
        //   "Block {} renderchildren: {}, children {:?}",
        //   block_id, render_children, block.children
        // );

        if render_children {
            result.push("\n");
            result.push(write_depth(depth + 1));

            let element = match block.view_type {
                ViewType::Document => "<ul class=\"list-document\">\n",
                ViewType::Bullet => "<ul class=\"list-bullet\">\n",
                ViewType::Numbered => "<ol class=\"list-numbered\">\n",
            };
            result.push(element);

            for child in &block.children {
                result.push(self.render_block_and_children(*child, depth + 2)?);
            }

            result.push(write_depth(depth + 1));

            let element = match block.view_type {
                ViewType::Document => "</ul>\n",
                ViewType::Bullet => "</ul>\n",
                ViewType::Numbered => "</ol>\n",
            };
            result.push(element);
        }

        if render_li {
            if render_children {
                result.push(write_depth(depth));
            }
            result.push("</li>");
        }
        result.push("\n");

        Ok(result)
    }

    pub fn render(&self) -> Result<String> {
        self.render_block_and_children(self.id, 0)
            .map(|results| results.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omit_blocks_with_only_unexported_links() {}

    #[test]
    fn render_table() {}

    #[test]
    fn table_omits_children() {}
}
