use crate::parse_string::{parse, Expression};
use crate::roam_edn::*;
use anyhow::{anyhow, Result};
use fxhash::FxHashMap;
use itertools::Itertools;
use rayon::prelude::*;
use std::borrow::Cow;
use std::fmt::Write as FmtWrite;
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
    if !s.is_empty() {
      self.writer.write_all("<li>".as_bytes())?;
      self.writer.write_all(s.as_bytes())?;
      self.writer.write_all("</li>\n".as_bytes())?;
    }

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

  fn render_block_uid(&self, s: &str) -> Result<String> {
    self
      .graph
      .block_from_uid(s)
      .map(|block| self.render_line(block).map(|(line, _)| line))
      .unwrap_or_else(|| Ok(String::new()))
  }

  fn descend_table_child(&self, row: Vec<Cow<'a, str>>, id: usize) -> Vec<Vec<Cow<'a, str>>> {
    self
      .graph
      .blocks
      .get(&id)
      .map(|block| {
        let rendered = self.render_line(block).unwrap();
        let mut row = row.clone();
        row.push(Cow::from(rendered.0));
        block
          .children
          .iter()
          .flat_map(|&child| self.descend_table_child(row.clone(), child))
          .collect::<Vec<_>>()
      })
      .unwrap_or_else(|| vec![row])
  }

  /// Given a block containing a table, render that table into markdown format
  fn render_table(&self, block: &Block) -> String {
    let rows = block
      .children
      .iter()
      .flat_map(|id| self.descend_table_child(Vec::new(), *id))
      .map(|row| {
        let mut output = String::from("<tr>\n");
        for cell in row {
          writeln!(output, "<td>{}</td>", cell).unwrap();
        }
        writeln!(output, "</tr>").unwrap();
        output
      })
      .collect::<String>();

    format!("<table>\n{}</table>", rows)
  }

  fn render_brace_directive(&self, block: &Block, s: &str) -> Cow<'a, str> {
    let value = match s {
      "table" => self.render_table(block),
      _ => format!("<pre>{}</pre>", s),
    };

    Cow::from(value)
  }

  fn render_expression(&self, block: &Block, e: Expression<'a>) -> Result<(Cow<'a, str>, bool)> {
    let rendered = match e {
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
      Expression::BlockRef(s) => Cow::from(self.render_block_uid(s)?),
      Expression::BraceDirective(s) => self.render_brace_directive(block, s),
      Expression::Attribute { .. } => Cow::from(""), // TODO
    };

    let render_children = match e {
      Expression::BraceDirective("table") => true,
      _ => true,
    };

    Ok((rendered, render_children))
  }

  fn render_line(&self, block: &'a Block) -> Result<(String, bool)> {
    parse(&block.string)
      .map_err(|e| anyhow!("Parse Error: {:?}", e))?
      .into_iter()
      .map(|e| self.render_expression(block, e))
      .fold(Ok((String::new(), true)), |acc, r| {
        acc.and_then(|(mut line, render_children)| {
          r.map(|r| {
            line.push_str(&r.0);
            (line, render_children && r.1)
          })
        })
      })
  }

  fn render_block_and_children(&mut self, block_id: usize) -> Result<()> {
    let block = self.graph.blocks.get(&block_id).unwrap();

    let (rendered, render_children) = self.render_line(block)?;
    self.write_line(&rendered)?;

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
