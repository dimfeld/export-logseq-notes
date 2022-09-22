use std::cell::Cell;

use crate::config::Config;
use crate::graph::{Block, Graph, ViewType};
use crate::html;
use crate::links;
use crate::parse_string::{parse, Expression};
use crate::string_builder::StringBuilder;
use crate::syntax_highlight;
use ahash::{AHashMap, AHashSet};
use eyre::{eyre, Result, WrapErr};
use serde::Serialize;
use smallvec::SmallVec;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncludeScope {
    Full,
    Partial(SmallVec<[usize; 4]>),
}

pub struct Page<'a, 'b> {
    pub id: usize,
    pub title: String,
    pub slug: &'a str,

    pub latest_found_edit_time: Cell<usize>,

    pub filter_tags: &'a [&'a str],
    pub graph: &'a Graph,
    pub config: &'a Config,
    pub pages_by_title: &'a AHashMap<String, IdSlugUid>,
    pub included_pages_by_title: &'a AHashMap<String, (&'a IdSlugUid, IncludeScope)>,
    pub included_pages_by_id: &'a AHashMap<usize, TitleSlugUid>,
    pub omitted_attributes: &'a AHashSet<&'a str>,

    pub include_scope: &'a IncludeScope,
    pub include_blocks_with_tags: &'a Vec<String>,
    pub include_blocks_with_prefix: &'a Vec<String>,
    pub highlighter: &'b syntax_highlight::Highlighter,
}

fn write_depth(depth: usize) -> String {
    "  ".repeat(depth)
}

fn render_opening_tag(tag: &str, class: &str) -> String {
    if class.is_empty() {
        format!("<{tag}>")
    } else {
        format!(r##"<{tag} class="{class}">"##)
    }
}

impl<'a, 'b> Page<'a, 'b> {
    fn link_if_allowed(&self, s: &'a str, omit_unexported_links: bool) -> StringBuilder<'a> {
        self.included_pages_by_title
            .get(s)
            .map(|(IdSlugUid { slug, .. }, _)| {
                let url = links::link_path(self.slug, slug, self.config.base_url.as_deref());
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
        seen_hashtags: &mut AHashSet<&'a str>,
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
                                    self.config.base_url.as_deref(),
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
                    .map_err(|e| eyre!("Parse Error: {}", e))
                    .and_then(|expressions| {
                        self.render_expressions(containing_block, expressions, seen_hashtags, false)
                    })
                    .map(|(sb, render_children)| (sb, true, render_children))
            }
        }
    }

    fn hashtag(&self, s: &'a str, dot: bool, omit_unexported_links: bool) -> StringBuilder<'a> {
        if self.filter_tags.contains(&s) || self.omitted_attributes.contains(&s) {
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
        seen_hashtags: &mut AHashSet<&'a str>,
    ) -> Result<StringBuilder<'a>> {
        self.graph
            .block_from_uid(s)
            .map(|block| {
                self.render_block_and_children(block, seen_hashtags, 0)
                    .map(|rendered| {
                        StringBuilder::Vec(vec![
                            StringBuilder::from(render_opening_tag(
                                "div",
                                self.config.class_block_embed.as_str(),
                            )),
                            rendered,
                            StringBuilder::from("</div>"),
                        ])
                    })
            })
            .unwrap_or(Ok(StringBuilder::Empty))
    }

    fn descend_table_child(
        &self,
        row: Vec<StringBuilder<'a>>,
        id: usize,
        seen_hashtags: &mut AHashSet<&'a str>,
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
        seen_hashtags: &mut AHashSet<&'a str>,
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
        seen_hashtags: &mut AHashSet<&'a str>,
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
        seen_hashtags: &mut AHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        self.render_expressions(block, e, seen_hashtags, false)
            .map(|(s, rc)| {
                (
                    StringBuilder::Vec(vec![
                        render_opening_tag(tag, class).into(),
                        s,
                        format!("</{tag}>").into(),
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
        seen_hashtags: &mut AHashSet<&'a str>,
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
        seen_hashtags: &mut AHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        if self.filter_tags.contains(&name) || self.omitted_attributes.get(name).is_some() {
            return Ok((StringBuilder::Empty, false, true));
        }

        self.render_expressions(block, contents, seen_hashtags, false)
            .map(|(s, rc)| {
                let output = StringBuilder::Vec(vec![
                    "<span>".into(),
                    // Attr name
                    render_opening_tag("span", self.config.class_attr_name.as_str()).into(),
                    html::escape(name).into(),
                    ":</span> ".into(),
                    // Attr value
                    render_opening_tag("span", self.config.class_attr_value.as_str()).into(),
                    s,
                    "</span></span>".into(),
                ]);

                (output, true, rc)
            })
    }

    fn render_expression(
        &self,
        block: &'a Block,
        e: Expression<'a>,
        seen_hashtags: &mut AHashSet<&'a str>,
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
            Expression::Bold(e) => self.render_style(
                block,
                "strong",
                self.config.class_bold.as_str(),
                e,
                seen_hashtags,
            )?,
            Expression::Italic(e) => self.render_style(
                block,
                "em",
                self.config.class_italic.as_str(),
                e,
                seen_hashtags,
            )?,
            Expression::Strike(e) => self.render_style(
                block,
                "del",
                self.config.class_strikethrough.as_str(),
                e,
                seen_hashtags,
            )?,
            Expression::Highlight(e) => self.render_style(
                block,
                "span",
                self.config.class_highlight.as_str(),
                e,
                seen_hashtags,
            )?,
            Expression::Latex(e) => (
                katex::render(e).with_context(|| e.to_string())?.into(),
                true,
                true,
            ),
            Expression::BlockQuote(e) => self.render_style(
                block,
                "blockquote",
                self.config.class_blockquote.as_str(),
                e,
                seen_hashtags,
            )?,
            Expression::Text(s) => (html::escape(s).into(), true, true),
            Expression::BlockRef(s) => self.render_block_ref(block, s, seen_hashtags)?,
            Expression::BraceDirective(s) => self.render_brace_directive(block, s, seen_hashtags),
            Expression::Table => (self.render_table(block, seen_hashtags), true, false),
            Expression::HRule => {
                let tag = if self.config.class_hr.is_empty() {
                    StringBuilder::from("<hr />")
                } else {
                    StringBuilder::from(format!(r##"<hr class="{}" />"##, self.config.class_hr))
                };

                (tag, true, true)
            }
            Expression::BlockEmbed(s) => {
                // let containing_page = self
                //     .graph
                //     .blocks
                //     .get(&block.containing_page)
                //     .and_then(|b| b.page_title.as_ref());
                // let referenced_page = self
                //     .graph
                //     .blocks_by_uid
                //     .get(s)
                //     .and_then(|id| self.graph.blocks.get(id))
                //     .and_then(|block| self.graph.blocks.get(&block.containing_page))
                //     .and_then(|b| b.page_title.as_ref());
                // println!("Page {containing_page:?} embedded block {s} in {referenced_page:?}");

                (self.render_block_embed(s, seen_hashtags)?, true, true)
            }
            Expression::PageEmbed(s) => {
                let page = if self.config.include_all_page_embeds {
                    self.pages_by_title.get(s)
                } else {
                    self.included_pages_by_title.get(s).map(|s| s.0)
                };

                // let containing_page = self
                //     .graph
                //     .blocks
                //     .get(&block.containing_page)
                //     .and_then(|b| b.page_title.as_ref());
                // println!("Page {containing_page:?} embedded page {s}");

                let result = page
                    .map(|IdSlugUid { id: block_id, .. }| {
                        let block = self.graph.blocks.get(block_id).unwrap();
                        self.render_block_and_children(block, seen_hashtags, 0).map(
                            |embedded_page| {
                                StringBuilder::Vec(vec![
                                    render_opening_tag(
                                        "div",
                                        self.config.class_page_embed_container.as_str(),
                                    )
                                    .into(),
                                    render_opening_tag(
                                        "div",
                                        self.config.class_page_embed_title.as_str(),
                                    )
                                    .into(),
                                    s.into(),
                                    "</div>".into(),
                                    render_opening_tag(
                                        "div",
                                        self.config.class_page_embed_content.as_str(),
                                    )
                                    .into(),
                                    embedded_page,
                                    "</div>\n</div>".into(),
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
        seen_hashtags: &mut AHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        let parsed = parse(self.graph.content_style, &block.string)
            .map_err(|e| eyre!("Parse Error: {:?}", e))?;

        let filter_links = self.config.filter_link_only_blocks
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
        seen_hashtags: &mut AHashSet<&'a str>,
    ) -> Result<(StringBuilder<'a>, bool)> {
        self.render_line_without_header(block, seen_hashtags)
            .map(|result| {
                let class = match block.heading {
                    1 => self.config.class_heading1.as_str(),
                    2 => self.config.class_heading2.as_str(),
                    3 => self.config.class_heading3.as_str(),
                    4 => self.config.class_heading4.as_str(),
                    _ => "",
                };

                if !class.is_empty() && !result.0.is_blank() {
                    (
                        StringBuilder::Vec(vec![
                            StringBuilder::from(format!(r##"<span class="{class}">"##)),
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
        seen_hashtags: &mut AHashSet<&'a str>,
        depth: usize,
    ) -> Result<StringBuilder<'a>> {
        let (rendered, render_children) = self.render_line(block, seen_hashtags)?;
        let render_children = render_children && !block.children.is_empty();

        if block.edit_time > self.latest_found_edit_time.get() {
            self.latest_found_edit_time.set(block.edit_time);
        }

        // println!(
        //     "Block {} renderchildren: {}, children {:?}, content {}",
        //     block.id, render_children, block.children, block.string
        // );

        if rendered.is_blank() && !render_children {
            return Ok(StringBuilder::Empty);
        }

        let render_li = depth > 0;

        let mut result = StringBuilder::with_capacity(9);
        result.push(write_depth(depth));

        if render_li {
            if block.uid.is_empty() {
                result.push(r##"<li>"##);
            } else {
                result.push(format!(r##"<li id="{id}">"##, id = block.uid));
            }
        }

        result.push(rendered);

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
                .filter_map(|id| {
                    let render = match (depth, self.include_scope) {
                        (0, IncludeScope::Full) => true,
                        (0, IncludeScope::Partial(p)) => p.contains(id),
                        (_, _) => true,
                    };

                    if render {
                        self.graph.blocks.get(id)
                    } else {
                        None
                    }
                })
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

    pub fn render(&self) -> Result<(String, AHashSet<&'a str>)> {
        let block = self.graph.blocks.get(&self.id).unwrap();
        let mut seen_hashtags: AHashSet<&'a str> = AHashSet::default();
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
