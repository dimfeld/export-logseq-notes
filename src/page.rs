use crate::graph::{Block, Graph, ViewType};
use crate::html;
use crate::links;
use crate::parse_string::{parse, Expression};
use crate::string_builder::StringBuilder;
use crate::syntax_highlight;
use anyhow::{anyhow, Result};
use fxhash::{FxHashMap, FxHashSet};
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
    pub slug: &'a str,

    pub filter_tags: &'a [&'a str],
    pub graph: &'a Graph,
    pub base_url: &'a Option<String>,
    pub filter_link_only_blocks: bool,
    pub pages_by_title: &'a FxHashMap<String, IdSlugUid>,
    pub included_pages_by_title: &'a FxHashMap<String, &'a IdSlugUid>,
    pub included_pages_by_id: &'a FxHashMap<usize, TitleSlugUid>,
    pub omitted_attributes: &'a FxHashSet<&'a str>,
    pub embed_unincluded_pages: bool,
    pub highlighter: &'b syntax_highlight::Highlighter,
}

fn write_depth(depth: usize) -> String {
    "  ".repeat(depth)
}

impl<'a, 'b> Page<'a, 'b> {
    fn link_if_allowed(&self, s: &'a str, omit_unexported_links: bool) -> StringBuilder<'a> {
        self.included_pages_by_title
            .get(s)
            .map(|IdSlugUid { slug, .. }| {
                let url = links::link_path(self.slug, slug, self.base_url.as_deref());
                StringBuilder::from(format!(
                    r##"<a href="{slug}">{title}</a>"##,
                    title = html::escape(s),
                    slug = html::escape(url.as_ref())
                ))
            })
            .unwrap_or_else(|| {
                if omit_unexported_links {
                    StringBuilder::Empty
                } else {
                    StringBuilder::from(html::escape(s))
                }
            })
    }

    fn render_block_ref(
        &self,
        containing_block: &'a Block,
        s: &'a str,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        let block = self.graph.block_from_uid(s);
        match block {
            Some(block) => {
                self.render_line_without_header(block, seen_hashtags)
                    .map(|(result, _)| {
                        match self.included_pages_by_id.get(&block.containing_page) {
                            Some(page) => {
                                // When the referenced page is exported, make this a link to the block.
                                let url = links::link_path(
                                    self.slug,
                                    &page.slug,
                                    self.base_url.as_deref(),
                                );
                                let linked = StringBuilder::Vec(vec![
                                    StringBuilder::from(format!(
                                        r##"<a class="block-ref" href="{page}#{block}">"##,
                                        page = url,
                                        block = block.uid
                                    )),
                                    result,
                                    StringBuilder::from("</a>"),
                                ]);

                                (linked, true, true)
                            }
                            None => (result, true, true),
                        }
                    })
            }
            None => {
                // Block ref syntax can also be expandable text. So if we don't match on a block then just render it.
                parse(self.graph.content_style, s)
                    .map_err(|e| anyhow!("Parse Error: {}", e))
                    .and_then(|expressions| {
                        self.render_expressions(containing_block, expressions, seen_hashtags, false)
                    })
                    .map(|(sb, render_children)| (sb, true, render_children))
            }
        }
    }

    fn hashtag(&self, s: &'a str, dot: bool, omit_unexported_links: bool) -> StringBuilder<'a> {
        if self.filter_tags.contains(&s) {
            // Don't render the primary export tags
            return StringBuilder::Empty;
        }

        let anchor = self.link_if_allowed(s, omit_unexported_links);
        if dot && !anchor.is_empty() {
            StringBuilder::Vec(vec![
                StringBuilder::from(format!("<span class=\"{}\">", s)),
                anchor,
                StringBuilder::from("</span>"),
            ])
        } else {
            anchor
        }
    }

    fn render_block_embed(
        &self,
        s: &str,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> Result<StringBuilder<'a>> {
        self
      .graph
      .block_from_uid(s)
      .map(|block| self.render_block_and_children(block, seen_hashtags, 0).map(|rendered| {
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
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> Vec<Vec<StringBuilder<'a>>> {
        self.graph
            .blocks
            .get(&id)
            .map(|block| {
                let rendered = self
                    .render_line_without_header(block, seen_hashtags)
                    .unwrap();
                let mut row = row.clone();
                row.push(rendered.0);

                if block.children.is_empty() {
                    vec![row]
                } else {
                    block
                        .children
                        .iter()
                        .flat_map(|&child| {
                            self.descend_table_child(row.clone(), child, seen_hashtags)
                        })
                        .collect::<Vec<_>>()
                }
            })
            .unwrap_or_else(|| vec![row])
    }

    /// Given a block containing a table, render that table into markdown format
    fn render_table(
        &self,
        block: &'a Block,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> StringBuilder<'a> {
        let rows = block
            .children
            .iter()
            .flat_map(|id| self.descend_table_child(Vec::new(), *id, seen_hashtags))
            .map(|row| {
                let mut output = StringBuilder::with_capacity(row.len() * 3 + 2);
                output.push("  <tr>\n");
                for cell in row {
                    output.push("    <td>");
                    output.push(cell);
                    output.push("</td>\n");
                }
                output.push("  </tr>\n");
                output
            })
            .collect::<Vec<StringBuilder>>();

        StringBuilder::Vec(vec![
            StringBuilder::from("\n<div class=\"roam-table\"><table><tbody>\n"),
            StringBuilder::from(rows),
            StringBuilder::from("</tbody></table></div>\n"),
        ])
    }

    fn render_brace_directive(
        &self,
        block: &'a Block,
        s: &'a str,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> (StringBuilder<'a>, bool, bool) {
        let (value, render_children) = match s {
            "table" => (self.render_table(block, seen_hashtags), false),
            _ => {
                if s.starts_with("query:") {
                    (StringBuilder::Empty, true)
                } else {
                    (
                        StringBuilder::from(format!("<pre>{}</pre>", html::escape(s))),
                        true,
                    )
                }
            }
        };

        (value, true, render_children)
    }

    fn render_style(
        &self,
        block: &'a Block,
        tag: &'a str,
        class: &'a str,
        e: Vec<Expression<'a>>,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        self.render_expressions(block, e, seen_hashtags, false)
            .map(|(s, rc)| {
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
                    true,
                    rc,
                )
            })
    }

    fn render_expressions(
        &self,
        block: &'a Block,
        e: Vec<Expression<'a>>,
        seen_hashtags: &mut FxHashSet<&'a str>,
        omit_unexported_links: bool,
    ) -> Result<(StringBuilder<'a>, bool)> {
        let num_exprs = e.len();
        e.into_iter()
            .map(|e| self.render_expression(block, e, seen_hashtags, omit_unexported_links))
            .fold(
                Ok((StringBuilder::with_capacity(num_exprs), false, true)),
                |acc, r| {
                    acc.and_then(|(mut line, should_render, render_children)| {
                        r.map(|(sb, must_render, this_render_children)| {
                            let should_render = should_render || (!sb.is_blank() && must_render);
                            line.push(sb);

                            (line, should_render, render_children && this_render_children)
                        })
                    })
                },
            )
            .map(|(line, should_render, render_children)| {
                if should_render {
                    (line, render_children)
                } else {
                    (StringBuilder::Empty, render_children)
                }
            })
    }

    fn render_attribute(
        &self,
        block: &'a Block,
        name: &'a str,
        contents: Vec<Expression<'a>>,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        if self.filter_tags.contains(&name) || self.omitted_attributes.get(name).is_some() {
            return Ok((StringBuilder::Empty, false, true));
        }

        self.render_expressions(block, contents, seen_hashtags, false)
            .map(|(s, rc)| {
                let mut output = StringBuilder::with_capacity(5);
                output.push(r##"<span><strong class="rm-attr-ref">"##);
                output.push(html::escape(name));
                output.push(":</strong> ");
                output.push(s);
                output.push("</span>");

                (output, true, rc)
            })
    }

    fn render_expression(
        &self,
        block: &'a Block,
        e: Expression<'a>,
        seen_hashtags: &mut FxHashSet<&'a str>,
        omit_unexported_links: bool,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        let rendered = match e {
            Expression::Hashtag(s, dot) => {
                seen_hashtags.insert(s);
                (self.hashtag(s, dot, omit_unexported_links), true, true)
            }
            Expression::Image { alt, url } => (
                format!(
                    r##"<img title="{alt}" src="{url}" />"##,
                    alt = html::escape(alt),
                    url = html::escape(url)
                )
                .into(),
                true,
                true,
            ),
            Expression::Todo { done } => (
                format!(
                    r##"<input type="checkbox" readonly="true" checked="{}" />"##,
                    done
                )
                .into(),
                false,
                true,
            ),
            Expression::Link(s) => (self.link_if_allowed(s, omit_unexported_links), true, true),
            Expression::MarkdownLink { title, url } => (
                format!(
                    r##"<a href="{url}">{title}</a>"##,
                    title = html::escape(title),
                    url = html::escape(url),
                )
                .into(),
                true,
                true,
            ),
            Expression::RawHyperlink(h) => (
                format!(r##"<a href="{url}">{url}</a>"##, url = html::escape(h),).into(),
                true,
                true,
            ),
            Expression::SingleBacktick(s) => (
                format!("<code>{}</code>", html::escape(s)).into(),
                true,
                true,
            ),
            Expression::TripleBacktick(s) => (
                format!("<pre><code>{}</code></pre>", self.highlighter.highlight(s)?).into(),
                true,
                true,
            ),
            Expression::Bold(e) => {
                self.render_style(block, "strong", "rm-bold", e, seen_hashtags)?
            }
            Expression::Italic(e) => {
                self.render_style(block, "em", "rm-italics", e, seen_hashtags)?
            }
            Expression::Strike(e) => {
                self.render_style(block, "del", "rm-strikethrough", e, seen_hashtags)?
            }
            Expression::Highlight(e) => {
                self.render_style(block, "span", "rm-highlight", e, seen_hashtags)?
            }
            Expression::Latex(e) => {
                let opts = katex::Opts::builder()
                    .output_type(katex::OutputType::HtmlAndMathml)
                    .build()
                    .unwrap();
                (katex::render_with_opts(e, &opts)?.into(), true, true)
            }
            Expression::BlockQuote(e) => {
                self.render_style(block, "blockquote", "rm-bq", e, seen_hashtags)?
            }
            Expression::Text(s) => (html::escape(s).into(), true, true),
            Expression::BlockRef(s) => self.render_block_ref(block, s, seen_hashtags)?,
            Expression::BraceDirective(s) => self.render_brace_directive(block, s, seen_hashtags),
            Expression::Table => (self.render_table(block, seen_hashtags), true, false),
            Expression::HRule => (r##"<hr class="rm-hr" />"##.into(), true, true),
            Expression::BlockEmbed(s) => (self.render_block_embed(s, seen_hashtags)?, true, true),
            Expression::PageEmbed(s) => {
                let page = if self.embed_unincluded_pages {
                    self.pages_by_title.get(s)
                } else {
                    self.included_pages_by_title.get(s).copied()
                };

                let result = page
                    .map(|IdSlugUid { id: block_id, .. }| {
                        let block = self.graph.blocks.get(block_id).unwrap();
                        self.render_block_and_children(block, seen_hashtags, 0).map(
                            |embedded_page| {
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
                            },
                        )
                    })
                    .unwrap_or(Ok(StringBuilder::Empty))?;
                (result, true, true)
            }
            Expression::Attribute { name, value } => {
                self.render_attribute(block, name, value, seen_hashtags)?
            }
        };

        Ok(rendered)
    }

    fn render_line_without_header(
        &self,
        block: &'a Block,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        let parsed = parse(self.graph.content_style, &block.string)
            .map_err(|e| anyhow!("Parse Error: {:?}", e))?;

        let filter_links = self.filter_link_only_blocks
            && parsed.iter().all(|e| match e {
                Expression::Link(_) => true,
                Expression::Hashtag(_, _) => true,
                Expression::Todo { .. } => true,
                Expression::Text(t) => t.trim().is_empty(),
                _ => false,
            });

        self.render_expressions(block, parsed, seen_hashtags, filter_links)
            .map(|(strings, render_children)| (strings, render_children))
    }

    fn render_line(
        &self,
        block: &'a Block,
        seen_hashtags: &mut FxHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        self.render_line_without_header(block, seen_hashtags)
            .map(|result| {
                if block.heading > 0 && !result.0.is_blank() {
                    (
                        StringBuilder::Vec(vec![
                            StringBuilder::from(format!(
                                "<span class=\"rm-heading-{}\">",
                                block.heading
                            )),
                            result.0,
                            StringBuilder::from("</span>"),
                        ]),
                        result.1,
                    )
                } else {
                    result
                }
            })
    }

    fn render_block_and_children(
        &self,
        block: &'a Block,
        seen_hashtags: &mut FxHashSet<&'a str>,
        depth: usize,
    ) -> Result<StringBuilder<'a>> {
        let (rendered, render_children) = self.render_line(block, seen_hashtags)?;
        let render_children = render_children && !block.children.is_empty();

        if rendered.is_blank() && !render_children {
            return Ok(StringBuilder::Empty);
        }

        let render_li = depth > 0;

        let mut result = StringBuilder::with_capacity(9);
        result.push(write_depth(depth));

        if render_li {
            result.push(format!(r##"<li id="{id}">"##, id = block.uid));
        }

        result.push(rendered);

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

            let mut children = block
                .children
                .iter()
                .filter_map(|id| self.graph.blocks.get(id))
                .collect::<Vec<_>>();
            if self.graph.block_explicit_ordering {
                children.sort_by_key(|b| b.order);
            }

            for child in &children {
                result.push(self.render_block_and_children(child, seen_hashtags, depth + 2)?);
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

    pub fn render(&self) -> Result<(String, FxHashSet<&'a str>)> {
        let block = self.graph.blocks.get(&self.id).unwrap();
        let mut seen_hashtags: FxHashSet<&'a str> = FxHashSet::default();
        self.render_block_and_children(block, &mut seen_hashtags, 0)
            .map(|results| (results.build(), seen_hashtags))
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
