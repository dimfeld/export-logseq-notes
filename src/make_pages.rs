use crate::parse_string::{parse, Expression};
use crate::roam_edn::*;
use anyhow::{anyhow, Result};
use fxhash::FxHashMap;
use itertools::Itertools;
use rayon::prelude::*;
use std::borrow::Cow;
use std::io::Write;
use std::path::Path;

pub struct Page<'a, W: Write> {
  pub id: usize,
  pub title: String,

  writer: W,
  graph: &'a Graph,
  included_pages: &'a FxHashMap<String, (usize, String)>,
}

impl<'a, W: Write> Page<'a, W> {
  pub fn write_line(&mut self, s: &str) -> Result<()> {
    self.writer.write_all("<li>".as_bytes())?;
    self.writer.write_all(s.as_bytes())?;
    self.writer.write_all("</li>\n".as_bytes())?;
    Ok(())
  }

  fn link_if_allowed(&self, s: &'a str) -> Cow<'a, str> {
    self
      .included_pages
      .get(s)
      .map(|(_, slug)| {
        Cow::from(format!(
          r##"<a href="{slug}">{title}</a>"##,
          title = s,
          slug = slug
        ))
      })
      .unwrap_or_else(|| Cow::from(s))
  }

  /// TODO
  fn render_brace_directive(&self, s: &str) -> Cow<'a, str> {
    Cow::from("")
  }

  fn render_expression(&self, e: Expression<'a>) -> Cow<'a, str> {
    match e {
      Expression::Hashtag(s) => self.link_if_allowed(s),
      Expression::Image { alt, url } => Cow::from(format!(
        r##"<img title="{alt}" src="{url}" />"##,
        alt = alt,
        url = url
      )),
      Expression::Link(s) => self.link_if_allowed(s),
      Expression::MarkdownLink { title, url } => Cow::from(format!(
        r##"<a href="{url}">{title}</a>"##,
        title = title,
        url = url
      )),
      Expression::SingleBacktick(s) => Cow::from(format!("<code>{}</code>", s)),
      // TODO Syntax highlighting
      Expression::TripleBacktick(s) => Cow::from(format!("<pre><code>{}</code></pre>", s)),
      Expression::Text(s) => Cow::from(s),
      Expression::BlockRef(_) => Cow::from(""), // TODO
      Expression::BraceDirective(s) => self.render_brace_directive(s),
      Expression::Attribute { .. } => Cow::from(""), // TODO
    }
  }

  fn render_line(&mut self, block: &'a Block) -> Result<bool> {
    let parsed = parse(&block.string).map_err(|e| anyhow!("Parse Error: {:?}", e))?;

    let line_values = parsed
      .into_iter()
      .map(|e| self.render_expression(e))
      .join("");

    self.write_line(&line_values)?;

    Ok(true)
  }

  fn render_block_and_children(&mut self, block_id: usize) -> Result<()> {
    let block = self.graph.blocks.get(&block_id).unwrap();

    let render_children = self.render_line(block)?;

    if render_children && !block.children.is_empty() {
      match block.view_type {
        ViewType::Document => self
          .writer
          .write_all(r##"<ul class="list-document">\n"##.as_bytes())?,
        ViewType::Bullet => self
          .writer
          .write_all(r##"<ul class="list-bullet">\n"##.as_bytes())?,
        ViewType::Numbered => self
          .writer
          .write_all(r##"<ol class="list-numbered">\n"##.as_bytes())?,
      }

      for child in &block.children {
        self.render_block_and_children(*child)?;
      }

      match block.view_type {
        ViewType::Document => self.writer.write_all(r##"</ul>\n"##.as_bytes())?,
        ViewType::Bullet => self.writer.write_all(r##"</ul>\n"##.as_bytes())?,
        ViewType::Numbered => self.writer.write_all(r##"</ol>\n"##.as_bytes())?,
      }
    }

    Ok(())
  }

  pub fn render(&mut self) -> Result<()> {
    self.render_block_and_children(self.id)
  }
}

/// Given a block containing a table, render that table into markdown format
fn render_table(block: &Block) -> String {
  unimplemented!();
}

fn title_to_slug(s: &str) -> String {
  s.split_whitespace()
    .map(|word| {
      word
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_digit(10))
        .flat_map(|c| c.to_lowercase())
        .collect::<String>()
    })
    .join("_")
}

pub fn make_pages<'a>(
  graph: &'a Graph,
  filter_tag: &str,
  output_dir: &Path,
) -> Result<Vec<(String, String)>> {
  let tag_node_id = *graph
    .titles
    .get(filter_tag)
    .ok_or_else(|| anyhow!("Could not find page with filter name {}", filter_tag))?;

  let included_pages = graph
    .blocks_with_reference(tag_node_id)
    .filter_map(|block| {
      let parsed = parse(&block.string).unwrap();

      let slug = match parsed.as_slice() {
        [Expression::Attribute { name, value }] => {
          if *name == filter_tag {
            value.iter().map(|e| e.plaintext()).join("")
          } else {
            title_to_slug(block.title.as_ref().unwrap())
          }
        }
        _ => title_to_slug(block.title.as_ref().unwrap()),
      };

      graph
        .blocks
        .get(&block.page)
        .map(|block| (block.title.clone().unwrap(), (block.id, slug)))
    })
    .collect::<FxHashMap<_, _>>();

  let pages = included_pages
    .par_iter()
    .map(|(title, (id, slug))| {
      let output_path = output_dir.join(slug);
      let mut writer = std::fs::File::create(output_path)?;

      let mut page = Page {
        id: *id,
        title: title.clone(),
        graph: &graph,
        included_pages: &included_pages,
        writer: &mut writer,
      };

      page.render()?;

      drop(page);
      writer.flush()?;

      Ok((title.clone(), slug.clone()))
    })
    .collect::<Result<Vec<_>>>();

  pages
}
