use crate::config::Config;
use crate::html;
use crate::parse_string::{parse, Expression};
use crate::roam_edn::*;
use crate::string_builder::StringBuilder;
use crate::syntax_highlight;
use anyhow::{anyhow, Result};
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use rayon::prelude::*;
use serde::Serialize;
use std::io::Write;

struct TitleAndSlug {
  title: String,
  slug: String,
}

pub struct Page<'a, 'b> {
  pub id: usize,
  pub title: String,

  filter_tag: &'a str,
  graph: &'a Graph,
  included_pages_by_title: &'a FxHashMap<String, (usize, String)>,
  included_pages_by_id: &'a FxHashMap<usize, TitleAndSlug>,
  highlighter: &'b syntax_highlight::Highlighter,
}

#[derive(Serialize, Debug)]
struct TemplateArgs<'a> {
  title: &'a str,
  body: &'a str,
  tags: Vec<&'a str>,
  created_time: usize,
  edited_time: usize,
}

fn write_depth(depth: usize) -> String {
  "  ".repeat(depth)
}

impl<'a, 'b> Page<'a, 'b> {
  fn link_if_allowed(&self, s: &'a str) -> StringBuilder<'a> {
    self
      .included_pages_by_title
      .get(s)
      .map(|(_, slug)| {
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
      Some(block) => self.render_line_without_header(block).map(|(result, _)| {
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
    self
      .graph
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
      Expression::SingleBacktick(s) => (format!("<code>{}</code>", html::escape(s)).into(), true),
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
        self
          .included_pages_by_title
          .get(s)
          .map(|(block_id, _)| {
            self
              .render_block_and_children(*block_id, 0)
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

  fn render_line_without_header(&self, block: &'a Block) -> Result<(StringBuilder<'a>, bool)> {
    let parsed = parse(&block.string).map_err(|e| anyhow!("Parse Error: {:?}", e))?;
    self.render_expressions(block, parsed)
  }

  fn render_line(&self, block: &'a Block) -> Result<(StringBuilder<'a>, bool)> {
    self.render_line_without_header(block).map(|result| {
      if block.heading > 0 {
        (
          StringBuilder::Vec(vec![
            StringBuilder::from(format!("<div class=\"rm-heading-{}\">", block.heading)),
            result.0,
            StringBuilder::from("</div>"),
          ]),
          result.1,
        )
      } else {
        result
      }
    })
  }

  fn render_block_and_children(&self, block_id: usize, depth: usize) -> Result<StringBuilder<'a>> {
    let block = self.graph.blocks.get(&block_id).unwrap();

    let (rendered, render_children) = self.render_line(block)?;
    let render_children = render_children && !block.children.is_empty();

    if rendered.is_empty() && !render_children {
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
    self
      .render_block_and_children(self.id, 0)
      .map(|results| results.build())
  }
}

fn title_to_slug(s: &str) -> String {
  s.split(|c: char| c.is_whitespace() || c == '/' || c == '-' || c == ':')
    .map(|word| {
      word
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_digit(10))
        .flat_map(|c| c.to_lowercase())
        .collect::<String>()
    })
    .filter(|w| !w.is_empty())
    .join("_")
}

// fn page_slug(config: &Config, graph: &Graph, page: &Block, attr_block: Option<&Block>) {
//   let attr_block = attr_block.unwrap_or_else(|| {
//     // TODO Find the block with the attr
//   })
// }

pub fn make_pages<'a, 'b>(
  graph: &'a Graph,
  handlebars: &handlebars::Handlebars,
  highlighter: &'b syntax_highlight::Highlighter,
  config: &Config,
) -> Result<Vec<(String, String)>> {
  let mut all_filter_tags = vec![config.include.to_string()];
  all_filter_tags.extend_from_slice(&config.also);

  let tag_node_ids = all_filter_tags
    .iter()
    .map(|tag| {
      graph
        .titles
        .get(tag)
        .copied()
        .ok_or_else(|| anyhow!("Could not find page with filter name {}", tag))
    })
    .collect::<Result<Vec<usize>>>()?;

  let exclude_tag_ids = config
    .exclude
    .iter()
    .map(|tag| {
      graph
        .titles
        .get(tag)
        .copied()
        .ok_or_else(|| anyhow!("Could not find page with excluded filter name {}", tag))
    })
    .collect::<Result<Vec<usize>>>()?;

  let excluded_page_ids = graph
    .blocks_with_references(&exclude_tag_ids)
    .map(|block| block.page)
    .chain(exclude_tag_ids.iter().copied())
    .collect::<FxHashSet<usize>>();

  // let block_iter = if config.include_all {
  //   graph.pages()
  // } else {

  // }

  let main_tag_uid = graph
    .titles
    .get(&config.include)
    .and_then(|tag| graph.blocks.get(tag))
    .map(|b| b.uid.as_str());

  let tags_attr_uid = graph
    .titles
    .get(&config.tags_attr)
    .and_then(|tag| graph.blocks.get(tag))
    .map(|b| b.uid.as_str())
    .ok_or_else(|| anyhow!("Could not find tags attribute {}", config.tags_attr))?;

  let included_pages_by_title = graph
    .blocks
    .iter()
    .filter_map(|(_, block)| {
      if !config.include_all && !block.refs.iter().any(|r| tag_node_ids.contains(r)) {
        return None;
      }

      let page = graph.blocks.get(&block.page)?;
      page.title.as_ref()?;

      if excluded_page_ids.get(&page.id).is_some() || (page.log_id > 0 && !config.allow_daily_notes)
      {
        println!("Excluded: {}", page.title.as_ref().unwrap());
        return None;
      }

      let slug = match main_tag_uid.and_then(|uid| page.referenced_attrs.get(uid)) {
        // The page sets the filename manually.
        Some(AttrValue::Str(s)) => s.clone(),
        // Otherwise generate it from the title.
        _ => title_to_slug(page.title.as_ref().unwrap()),
      };

      Some((page.title.clone().unwrap(), (page.id, slug)))
    })
    .collect::<FxHashMap<_, _>>();

  let included_pages_by_id = included_pages_by_title
    .iter()
    .map(|(title, (id, slug))| {
      (
        *id,
        TitleAndSlug {
          title: title.clone(),
          slug: slug.clone(),
        },
      )
    })
    .collect::<FxHashMap<_, _>>();

  let pages = included_pages_by_title
    .par_iter()
    .map(|(title, (id, slug))| {
      let mut output_path = config.output.join(slug);
      output_path.set_extension(&config.extension);

      let page = Page {
        id: *id,
        title: title.clone(),
        graph: &graph,
        filter_tag: &config.include,
        included_pages_by_title: &included_pages_by_title,
        included_pages_by_id: &included_pages_by_id,
        highlighter,
      };

      let rendered = page.render()?;

      let block = graph.blocks.get(id).unwrap();

      let tags = block
        .referenced_attrs
        .get(tags_attr_uid)
        .and_then(|attr| match attr {
          AttrValue::Str(s) => Some(s.split(",").map(|s| s.trim()).collect::<Vec<_>>()),
          _ => None,
        })
        .unwrap_or_else(Vec::new);

      let template_data = TemplateArgs {
        title,
        body: &rendered,
        tags,
        created_time: block.create_time,
        edited_time: block.edit_time,
      };
      let full_page = handlebars.render("page", &template_data)?;

      let mut writer = std::fs::File::create(output_path)?;
      writer.write_all(full_page.as_bytes())?;
      writer.flush()?;

      println!("Wrote: \"{}\" to {}", title, slug);

      Ok((title.clone(), slug.clone()))
    })
    .collect::<Result<Vec<_>>>();

  pages
}
