use std::{
    borrow::Cow,
    cell::Cell,
    path::{Path, PathBuf},
};

use ahash::{HashMap, HashSet};
use eyre::{eyre, Result, WrapErr};
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use serde::Serialize;
use urlencoding::encode;

use crate::{
    config::Config,
    graph::{Block, BlockInclude, Graph, ListType, ViewType},
    html,
    image::{image_full_path, ImageInfo},
    parse_string::{parse, Expression},
    string_builder::StringBuilder,
    syntax_highlight,
};

pub struct TitleSlugUid {
    pub title: String,
    pub slug: String,
    pub uid: String,
    pub include: bool,
    pub allow_embed: bool,
}

pub struct IdSlugUid {
    pub id: usize,
    pub output_title: String,
    pub slug: String,
    pub uid: String,
    pub include: bool,
    pub allow_embed: bool,
}

#[derive(Serialize)]
pub struct ManifestItem {
    pub slug: String,
    pub title: String,
    pub uid: String,
}

pub struct Page<'a> {
    pub id: usize,
    pub title: String,
    pub slug: &'a str,

    pub latest_found_edit_time: Cell<u64>,

    pub graph: &'a Graph,
    pub base_dir: &'a Path,
    pub path: PathBuf,
    pub config: &'a Config,
    pub heading_delta: isize,
    pub pages_by_title: &'a HashMap<String, IdSlugUid>,
    pub pages_by_filename_title: &'a HashMap<String, String>,
    pub pages_by_id: &'a HashMap<usize, TitleSlugUid>,
    pub omitted_attributes: &'a HashSet<&'a str>,
    pub highlighter: &'a syntax_highlight::Highlighter,
    pub handlebars: &'a handlebars::Handlebars<'a>,

    pub picture_template_key: &'a str,
    pub image_info: &'a HashMap<String, ImageInfo>,
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

impl<'a> Page<'a> {
    /// Render text as HTML, escaping HTML reserved characters but not performing any other
    /// transformations. This is useful when rendering code into code blocks.
    fn render_plain_text<'tx>(&self, text: &'tx str) -> Cow<'tx, str> {
        html::escape(text)
    }

    /// Render text as HTML, performing any enabled transformations such as converting
    /// -- into an emdash.
    fn render_text<'tx>(&self, text: &'tx str) -> Cow<'tx, str> {
        static TWODASH: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(^|[^-])--([^-]|$)"#).unwrap());

        let escaped = self.render_plain_text(text);

        let with_emdash = if self.config.convert_emdash && escaped.contains("--") {
            let rep = TWODASH
                .replace_all(&escaped, |caps: &Captures| {
                    format!("{}&mdash;{}", &caps[1], &caps[2])
                })
                .into_owned();
            // Not ideal but this is an easy way to break the borrow checker's reliance on
            // `escaped`. We already checked that `escaped` contains `--` so the false positive
            // rate should be low.
            Cow::from(rep)
        } else {
            escaped
        };

        with_emdash
    }

    fn lookup_page_by_title(&self, title: &str) -> Option<&IdSlugUid> {
        if let Some(page) = self.pages_by_title.get(title) {
            return Some(page);
        }

        if let Some(lookup_title) = self.pages_by_filename_title.get(title) {
            self.pages_by_title.get(lookup_title)
        } else {
            None
        }
    }

    fn link_if_allowed_with_label(
        &self,
        page: &'a str,
        label: Option<&'a str>,
        omit_unexported_links: bool,
    ) -> StringBuilder<'a> {
        self.lookup_page_by_title(page)
            .filter(|p| p.include)
            .map(
                |IdSlugUid {
                     slug, output_title, ..
                 }| {
                    let output_label = label.unwrap_or(output_title.as_str());
                    StringBuilder::from(format!(
                        r##"<a href="{slug}">{title}</a>"##,
                        title = self.render_text(output_label),
                        slug = html::escape(slug)
                    ))
                },
            )
            .unwrap_or_else(|| {
                if omit_unexported_links {
                    StringBuilder::Empty
                } else {
                    StringBuilder::from(self.render_text(label.unwrap_or(page)))
                }
            })
    }

    fn link_if_allowed(&self, s: &'a str, omit_unexported_links: bool) -> StringBuilder<'a> {
        self.link_if_allowed_with_label(s, None, omit_unexported_links)
    }

    fn render_block_ref(
        &'a self,
        containing_block: &'a Block,
        s: &'a str,
        first: bool,
    ) -> Result<(StringBuilder<'a>, bool, bool)> {
        let block = self.graph.block_from_uid(s);
        match block {
            Some(block) => {
                self.render_line_without_header(block).map(|(result, _)| {
                    match self
                        .pages_by_id
                        .get(&block.containing_page)
                        .filter(|p| p.include)
                    {
                        Some(page) => {
                            // When the referenced page is exported, make this a link to the block.
                            let linked = StringBuilder::Vec(vec![
                                StringBuilder::from(format!(
                                    r##"<a class="block-ref" href="{page}#{block}">"##,
                                    page = &page.slug,
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
                        self.render_expressions(containing_block, &expressions, first, false)
                    })
                    .map(|(sb, render_children)| (sb, true, render_children))
            }
        }
    }

    fn hashtag(&self, s: &'a str, dot: bool, omit_unexported_links: bool) -> StringBuilder<'a> {
        let anchor = self.link_if_allowed(s, omit_unexported_links);
        if dot && !anchor.is_empty() {
            StringBuilder::Vec(vec![
                StringBuilder::from(format!("<span class=\"{s}\">")),
                anchor,
                StringBuilder::from("</span>"),
            ])
        } else {
            anchor
        }
    }

    fn render_image(&self, url: &str, alt: &str) -> Result<StringBuilder> {
        let image_info = image_full_path(self.base_dir, &self.path, url)
            .and_then(|path| self.image_info.get(path.to_string_lossy().as_ref()));

        match image_info {
            Some(info) => {
                let mut data = handlebars::to_json(&info.data);

                // Find a format that would be supported by browsers that don't support the
                // <picture> tag.
                let fallback = info
                    .data
                    .output
                    .iter()
                    .find(|o| o.format == "jpg" || o.format == "png")
                    .unwrap_or(&info.data.output[0]);

                data["fallback"] = handlebars::to_json(fallback);
                data["alt"] = handlebars::to_json(alt);

                let rendered = self.handlebars.render(self.picture_template_key, &data)?;
                Ok(rendered.into())
            }
            None => Ok(format!(
                r##"<img alt="{alt}" src="{url}" />"##,
                alt = html::escape(alt),
                url = html::escape(url)
            )
            .into()),
        }
    }

    fn render_video(&self, url: &str) -> StringBuilder {
        // Not great with fixed size
        StringBuilder::from(format!(
            r##"<video controls src="{u}" width="800" height="450"></video>"##,
            u = encode(url)
        ))
    }

    fn render_block_embed(&'a self, s: &'a str) -> Result<StringBuilder<'a>> {
        self.graph
            .block_from_uid(s)
            .map(|block| {
                self.render_block_and_children(block, ViewType::default_view_type(), 0)
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
        &'a self,
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

                if block.children.is_empty() {
                    vec![row]
                } else {
                    block
                        .children
                        .iter()
                        .flat_map(|&child| self.descend_table_child(row.clone(), child))
                        .collect::<Vec<_>>()
                }
            })
            .unwrap_or_else(|| vec![row])
    }

    /// Given a block containing a table, render that table into markdown format
    fn render_table(&'a self, block: &'a Block) -> StringBuilder<'a> {
        let rows = block
            .children
            .iter()
            .flat_map(|id| self.descend_table_child(Vec::new(), *id))
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
        &'a self,
        block: &'a Block,
        s: &'a str,
    ) -> (StringBuilder<'a>, bool, bool) {
        let (value, render_children) = match s {
            "table" => (self.render_table(block), false),
            _ => {
                if s.starts_with("query:") || s.starts_with("renderer ") {
                    (StringBuilder::Empty, true)
                } else {
                    (
                        StringBuilder::from(format!("<pre>{}</pre>", self.render_plain_text(s))),
                        true,
                    )
                }
            }
        };

        (value, true, render_children)
    }

    fn render_style<'ex>(
        &'a self,
        block: &'a Block,
        tag: &'a str,
        class: &'a str,
        e: &'ex [Expression<'a>],
    ) -> Result<(StringBuilder, bool, bool)>
    where
        'a: 'ex,
    {
        self.render_expressions(block, e, false, false)
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

    fn render_expressions<'exs>(
        &'a self,
        block: &'a Block,
        e: &'exs [Expression<'a>],
        first: bool,
        omit_unexported_links: bool,
    ) -> Result<(StringBuilder<'a>, bool)>
    where
        'a: 'exs,
    {
        let num_exprs = e.len();
        e.iter()
            .enumerate()
            .map(|(i, e)| self.render_expression(block, e, first && i == 0, omit_unexported_links))
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

    fn render_attribute<'ex>(
        &'a self,
        block: &'a Block,
        name: &'a str,
        contents: &'ex [Expression<'a>],
        first: bool,
    ) -> Result<(StringBuilder, bool, bool)>
    where
        'a: 'ex,
    {
        if self.omitted_attributes.get(name).is_some() {
            return Ok((StringBuilder::Empty, false, true));
        }

        self.render_expressions(block, contents, false, false)
            .map(|(s, rc)| {
                let output = StringBuilder::Vec(vec![
                    if first { "<span>" } else { "<br /><span>" }.into(),
                    // Attr name
                    render_opening_tag("span", self.config.class_attr_name.as_str()).into(),
                    self.render_plain_text(name).into(),
                    ":</span> ".into(),
                    // Attr value
                    render_opening_tag("span", self.config.class_attr_value.as_str()).into(),
                    s,
                    "</span></span>".into(),
                ]);

                (output, true, rc)
            })
    }

    fn render_expression<'ex>(
        &'a self,
        block: &'a Block,
        e: &'ex Expression<'a>,
        first: bool,
        omit_unexported_links: bool,
    ) -> Result<(StringBuilder<'a>, bool, bool)>
    where
        'a: 'ex,
    {
        let rendered = match e {
            Expression::Hashtag(s, dot) => {
                (self.hashtag(s, *dot, omit_unexported_links), true, true)
            }
            Expression::RawHtml(s) => (StringBuilder::String((*s).into()), true, true),
            Expression::Image { alt, url } => (self.render_image(url, alt)?, true, true),
            Expression::Video { url } => (self.render_video(url), true, true),
            Expression::Todo { done } => {
                let done = if *done { "checked" } else { "" };

                (
                    format!(r##"<input type="checkbox" disabled {done} />"##,).into(),
                    false,
                    true,
                )
            }
            Expression::Link(s) => (self.link_if_allowed(s, omit_unexported_links), true, true),
            Expression::MarkdownInternalLink { page, label } => (
                self.link_if_allowed_with_label(page, Some(label), false),
                true,
                true,
            ),
            Expression::MarkdownExternalLink { title, url } => (
                format!(
                    r##"<a href="{url}">{title}</a>"##,
                    title = self.render_text(title),
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
                format!("<code>{}</code>", self.render_plain_text(s)).into(),
                true,
                true,
            ),
            Expression::TripleBacktick(s) => (
                format!("<pre><code>{}</code></pre>", self.highlighter.highlight(s)?).into(),
                true,
                true,
            ),
            Expression::Bold(e) => {
                self.render_style(block, "strong", self.config.class_bold.as_str(), e)?
            }
            Expression::Italic(e) => {
                self.render_style(block, "em", self.config.class_italic.as_str(), e)?
            }
            Expression::Strike(e) => {
                self.render_style(block, "del", self.config.class_strikethrough.as_str(), e)?
            }
            Expression::Highlight(e) => {
                self.render_style(block, "span", self.config.class_highlight.as_str(), e)?
            }
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
            )?,
            Expression::Text(s) => (self.render_text(s).into(), true, true),
            Expression::BlockRef(s) => self.render_block_ref(block, s, first)?,
            Expression::BraceDirective(s) => self.render_brace_directive(block, s),
            Expression::Table => (self.render_table(block), true, false),
            Expression::HRule => {
                let tag = if self.config.class_hr.is_empty() {
                    StringBuilder::from("<hr />")
                } else {
                    StringBuilder::from(format!(r##"<hr class="{}" />"##, self.config.class_hr))
                };

                (tag, true, true)
            }
            Expression::BlockEmbed(s) => (self.render_block_embed(s)?, true, true),
            Expression::PageEmbed(s) => {
                let page = self.lookup_page_by_title(*s).filter(|p| p.allow_embed);

                let result = page
                    .map(|IdSlugUid { id: block_id, .. }| {
                        let block = self.graph.blocks.get(block_id).unwrap();
                        self.render_block_and_children(block, ViewType::default_view_type(), 0)
                            .map(|embedded_page| {
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
                                    (*s).into(),
                                    "</div>".into(),
                                    render_opening_tag(
                                        "div",
                                        self.config.class_page_embed_content.as_str(),
                                    )
                                    .into(),
                                    embedded_page,
                                    "</div>\n</div>".into(),
                                ])
                            })
                    })
                    .unwrap_or(Ok(StringBuilder::Empty))?;
                (result, true, true)
            }
            Expression::Attribute { name, value } => {
                self.render_attribute(block, name, value, first)?
            }
        };

        Ok(rendered)
    }

    fn render_line_without_header(&'a self, block: &'a Block) -> Result<(StringBuilder<'a>, bool)> {
        let parsed = block.contents.borrow_parsed();
        let filter_links = self.config.filter_link_only_blocks
            && parsed.iter().all(|e| match e {
                Expression::Link(_) => true,
                Expression::Hashtag(_, _) => true,
                Expression::Todo { .. } => true,
                Expression::Text(t) => t.trim().is_empty(),
                _ => false,
            });

        self.render_expressions(block, parsed, true, filter_links)
            .map(|(strings, render_children)| (strings, render_children))
    }

    fn render_line(&'a self, block: &'a Block) -> Result<(StringBuilder<'a>, bool)> {
        self.render_line_without_header(block).map(|result| {
            let heading_level = if block.heading > 0 {
                std::cmp::max(1, block.heading as isize + self.heading_delta)
            } else {
                0
            };

            let (element, class) = match heading_level {
                1 => ("h1", self.config.class_heading1.as_str()),
                2 => ("h2", self.config.class_heading2.as_str()),
                3 => ("h3", self.config.class_heading3.as_str()),
                4 => ("h4", self.config.class_heading4.as_str()),
                _ => ("", ""),
            };

            if result.0.is_blank() || element.is_empty() {
                return result;
            }

            (
                StringBuilder::Vec(vec![
                    StringBuilder::from(render_opening_tag(element, class)),
                    result.0,
                    StringBuilder::from(format!("</{element}>")),
                ]),
                result.1,
            )
        })
    }

    fn render_block_and_children(
        &'a self,
        block: &'a Block,
        inherited_view_type: ViewType,
        depth: usize,
    ) -> Result<StringBuilder<'a>> {
        let (rendered, include_type_renders_li, render_child_container, render_children) =
            match block.include_type {
                BlockInclude::Exclude => return Ok(StringBuilder::Empty),
                BlockInclude::JustBlock => {
                    let (rendered, _) = self.render_line(block)?;
                    (rendered, true, false, false)
                }
                BlockInclude::AndChildren | BlockInclude::IfChildrenPresent => {
                    let (rendered, render_children) = self.render_line(block)?;
                    (rendered, true, true, render_children)
                }
                BlockInclude::OnlyChildren => (StringBuilder::Empty, false, false, true),
            };

        let increase_depth = block.include_type != BlockInclude::OnlyChildren;
        let render_children = render_children && !block.children.is_empty();

        // This doesn't completely match the Logseq behavior, which allows co-mingling of ordered
        // and unordered lists in the children of a single block.
        let has_numbered_list_child = render_child_container
            .then(|| {
                block.children.iter().any(|c| {
                    self.graph
                        .blocks
                        .get(&c)
                        .map(|b| b.this_block_list_type == ListType::Number)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        let view_type = block.view_type.resolve_with_parent(inherited_view_type);

        let child_container = match (render_child_container, has_numbered_list_child, view_type) {
            (false, _, _) => None,
            (true, false, ViewType::Document) => None,
            (true, false, ViewType::Bullet) => Some(("<ul class=\"list-bullet\">\n", "</ul>")),
            (true, true, _) | (true, false, ViewType::Numbered) => {
                Some(("<ol class=\"list-numbered\">\n", "</ol>"))
            }
            (true, false, ViewType::Inherit) => panic!("ViewType should never resolve to Inherit"),
        };

        if block.edit_time > self.latest_found_edit_time.get() {
            self.latest_found_edit_time.set(block.edit_time);
        }

        // println!("Block {block:?} renderchildren: {render_children}",);

        if rendered.is_blank() && !render_children {
            return Ok(StringBuilder::Empty);
        }

        let parent_is_list = depth > 0 && inherited_view_type != ViewType::Document;
        // Render the li if we're in an include type that renders this block
        // if we're rendering this inside a list
        let render_li = (include_type_renders_li && parent_is_list)
            || block.this_block_list_type == ListType::Number;

        let mut result = StringBuilder::with_capacity(9);
        result.push(write_depth(depth));

        let render_content_element = view_type == ViewType::Document
            && (block.heading == 0 || block.content_element.is_some())
            && !render_li
            && !rendered.is_blank()
            // Really bad hack. Need something better but it suffices
            // for the moment.
            && !rendered.starts_with("<pre");

        let extra_classes = block.extra_classes.join(" ");

        // Figure out where to put the extra classes, if any.
        // If we have a wrapper element then prefer that.
        // If we don't have a wrapper element but we do have an li, then put it there.
        // If we have neither, then force a div wrapper element and place the classes there.
        let (wrapper_extra_classes, li_extra_classes) = match (
            extra_classes.is_empty(),
            block.wrapper_element.is_some(),
            render_li,
        ) {
            // No extra classes
            (true, _, _) => ("", ""),
            // Extra classes with a wrapper element
            (false, true, _) => (extra_classes.as_str(), ""),
            // Extra classes with neither, so we'll force a wrapper
            (false, false, false) => (extra_classes.as_str(), ""),
            // Extra classes with an <li>
            (false, false, true) => ("", extra_classes.as_str()),
        };

        if render_li {
            if block.uid.is_empty() {
                result.push(render_opening_tag("li", li_extra_classes));
            } else if li_extra_classes.is_empty() {
                result.push(format!(r##"<li id="{id}">"##, id = block.uid));
            } else {
                result.push(format!(
                    r##"<li id="{id}" class="{li_extra_classes}">"##,
                    id = block.uid
                ));
            }
        }

        let wrapper_element = if wrapper_extra_classes.is_empty() {
            block.wrapper_element.as_deref().unwrap_or_default()
        } else {
            block.wrapper_element.as_deref().unwrap_or("div")
        };

        if !wrapper_element.is_empty() {
            result.push(render_opening_tag(wrapper_element, wrapper_extra_classes));
        }

        if render_content_element {
            if block.uid.is_empty() {
                match block.content_element.as_deref() {
                    Some(e) => result.push(format!("<{e}>")),
                    None => result.push("<p>"),
                };
            } else {
                let element_name = block.content_element.as_deref().unwrap_or("p");
                result.push(format!(r##"<{element_name} id="{id}">"##, id = block.uid));
            }
        }

        result.push(rendered);

        // For a document view type, we don't want to render the children inside this paragraph,
        // since we are flattening the structure. So close it here and let the children render on
        // their own.
        if render_content_element {
            match block.content_element.as_deref() {
                Some(e) => result.push(format!("</{e}>")),
                None => result.push("</p>"),
            };
        }

        let mut child_had_content = false;
        if render_children {
            let (child_container_depth, child_depth) = if increase_depth {
                (depth + 1, depth + 2)
            } else {
                (depth, depth)
            };

            result.push("\n");

            if let Some((child_container_start, _)) = child_container.as_ref() {
                result.push(write_depth(child_container_depth));
                result.push(*child_container_start);
            }

            let mut children = block
                .children
                .iter()
                .filter_map(|id| self.graph.blocks.get(id))
                .collect::<Vec<_>>();
            if self.graph.block_explicit_ordering {
                children.sort_by_key(|b| b.order);
            }

            for child in &children {
                let child_content =
                    self.render_block_and_children(child, view_type, child_depth)?;

                if !child_had_content && !child_content.is_blank() {
                    child_had_content = true;
                }

                result.push(child_content);
            }

            if let Some((_, child_container_end)) = child_container.as_ref() {
                result.push(write_depth(child_container_depth));
                result.push(*child_container_end);
            }
        }

        if block.include_type == BlockInclude::IfChildrenPresent && !child_had_content {
            return Ok(StringBuilder::Empty);
        }

        if !wrapper_element.is_empty() {
            result.push(format!("</{wrapper_element}>"));
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

    pub fn render(&'a self) -> Result<String> {
        let block = self.graph.blocks.get(&self.id).unwrap();
        self.render_block_and_children(block, ViewType::default_view_type(), 0)
            .map(|results| (results.build()))
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
