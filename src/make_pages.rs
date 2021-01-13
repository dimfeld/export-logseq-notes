use crate::html;
use crate::parse_string::{parse, Expression};
use crate::roam_edn::*;
use crate::syntax_highlight;
use anyhow::{anyhow, Result};
use fxhash::FxHashMap;
use itertools::Itertools;
use rayon::prelude::*;
use std::borrow::Cow;
use std::fmt::Write as FmtWrite;
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct Page<'a, 'b, W: Write> {
  pub id: usize,
  pub title: String,

  writer: W,
  graph: &'a Graph,
  included_pages: &'a FxHashMap<String, (usize, String)>,
  highlighter: &'b syntax_highlight::Highlighter,
}

impl<'a, 'b, W: Write> Page<'a, 'b, W> {
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
          title = html::escape(s),
          slug = html::escape(slug)
        ))
      })
      .unwrap_or_else(|| html::escape(s))
  }

  fn hashtag(&self, s: &'a str, dot: bool) -> Cow<'a, str> {
    let anchor = self.link_if_allowed(s);
    if dot {
      Cow::from(format!(
        "<span class=\"{s}\">{anchor}</span>",
        s = s,
        anchor = anchor
      ))
    } else {
      anchor
    }
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

  fn render_brace_directive(&self, block: &Block, s: &str) -> (Cow<'a, str>, bool) {
    let (value, render_children) = match s {
      "table" => (self.render_table(block), false),
      _ => (format!("<pre>{}</pre>", html::escape(s)), true),
    };

    (Cow::from(value), render_children)
  }

  fn render_style(
    &self,
    block: &Block,
    tag: &str,
    class: &str,
    e: Vec<Expression<'a>>,
  ) -> Result<(Cow<'a, str>, bool)> {
    self.render_expressions(block, e).map(|(s, rc)| {
      (
        Cow::from(format!(
          r##"<{tag} class="{class}">{contents}</{tag}>"##,
          tag = tag,
          class = class,
          contents = s,
        )),
        rc,
      )
    })
  }

  fn render_expressions(&self, block: &Block, e: Vec<Expression<'a>>) -> Result<(String, bool)> {
    e.into_iter()
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

  fn render_attribute(
    &self,
    block: &Block,
    name: &str,
    contents: Vec<Expression<'a>>,
  ) -> Result<(Cow<'a, str>, bool)> {
    self.render_expressions(block, contents).map(|(s, rc)| {
      let output = format!(
        r##"<span><strong class="rm-attr-ref">{name}:</strong>{s}</span> "##,
        name = html::escape(name),
        s = s
      );
      (Cow::from(output), rc)
    })
  }

  fn render_expression(&self, block: &Block, e: Expression<'a>) -> Result<(Cow<'a, str>, bool)> {
    let (rendered, render_children) = match e {
      Expression::Hashtag(s, dot) => (self.hashtag(s, dot), true),
      Expression::Image { alt, url } => (
        Cow::from(format!(
          r##"<img title="{alt}" src="{url}" />"##,
          alt = html::escape(alt),
          url = html::escape(url)
        )),
        true,
      ),
      Expression::Link(s) => (self.link_if_allowed(s), true),
      Expression::MarkdownLink { title, url } => (
        Cow::from(format!(
          r##"<a href="{url}">{title}</a>"##,
          title = html::escape(title),
          url = html::escape(url),
        )),
        true,
      ),
      Expression::SingleBacktick(s) => {
        (Cow::from(format!("<code>{}</code>", html::escape(s))), true)
      }
      Expression::TripleBacktick(s) => (
        Cow::from(format!(
          "<pre><code>{}</code></pre>",
          self.highlighter.highlight(s)
        )),
        true,
      ),
      Expression::Bold(e) => self.render_style(block, "strong", "rm-bold", e)?,
      Expression::Italic(e) => self.render_style(block, "em", "rm-italics", e)?,
      Expression::Strike(e) => self.render_style(block, "del", "rm-strikethrough", e)?,
      Expression::Highlight(e) => self.render_style(block, "span", "rm-highlight", e)?,
      Expression::Text(s) => (html::escape(s), true),
      // TODO This is wrong. Render a link to the block instead
      Expression::BlockRef(s) => (Cow::from(self.render_block_uid(s)?), true),
      Expression::BraceDirective(s) => self.render_brace_directive(block, s),
      Expression::Table => (Cow::from(self.render_table(block)), false),
      Expression::HRule => (Cow::from(r##"<hr class="rm-hr" />"##), true),
      Expression::BlockEmbed(s) => (Cow::from(self.render_block_uid(s)?), true),
      Expression::PageEmbed(s) => {
        (Cow::from(s), true)
        // TODO This writes to the writer instead of returning a string.
        //      Need to deal with that.
        //   (
        //   Cow::from(
        //     self
        //       .graph
        //       .blocks_by_uid
        //       .get(s)
        //       .map(|&block| self.render_block_and_children(block).map(|(line, _)| line))
        //       .unwrap_or_else(|| String::new()),
        //   ),
        //   true,
        // )
      }
      Expression::Attribute { name, value } => self.render_attribute(block, name, value)?, // TODO
    };

    Ok((rendered, render_children))
  }

  fn render_line(&self, block: &'a Block) -> Result<(String, bool)> {
    let parsed = parse(&block.string).map_err(|e| anyhow!("Parse Error: {:?}", e))?;

    self.render_expressions(block, parsed)
  }

  fn render_block_and_children(&mut self, block_id: usize) -> Result<()> {
    let block = self.graph.blocks.get(&block_id).unwrap();

    let (rendered, render_children) = self.render_line(block)?;
    self.write_line(&rendered)?;

    println!(
      "Block {} renderchildren: {}, children {:?}",
      block_id, render_children, block.children
    );

    if render_children && !block.children.is_empty() {
      match block.view_type {
        ViewType::Document => self
          .writer
          .write_all("<li><ul class=\"list-document\">".as_bytes())?,
        ViewType::Bullet => self
          .writer
          .write_all("<li><ul class=\"list-bullet\">".as_bytes())?,
        ViewType::Numbered => self
          .writer
          .write_all("<li><ol class=\"list-numbered\">".as_bytes())?,
      }

      for child in &block.children {
        self.render_block_and_children(*child)?;
      }

      match block.view_type {
        ViewType::Document => self.writer.write_all("</ul></li>\n".as_bytes())?,
        ViewType::Bullet => self.writer.write_all("</ul></li>\n".as_bytes())?,
        ViewType::Numbered => self.writer.write_all("</ol></li>\n".as_bytes())?,
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

pub fn make_pages<'a, 'b>(
  graph: &'a Graph,
  highlighter: &'b syntax_highlight::Highlighter,
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

      let page = graph.blocks.get(&block.page)?;

      let slug = match parsed.as_slice() {
        [Expression::Attribute { name, value }] => {
          if *name == filter_tag {
            value.iter().map(|e| e.plaintext()).join("")
          } else {
            title_to_slug(page.title.as_ref().unwrap())
          }
        }
        _ => title_to_slug(page.title.as_ref().unwrap()),
      };

      Some((page.title.clone().unwrap(), (page.id, slug)))
    })
    .collect::<FxHashMap<_, _>>();

  let pages = included_pages
    .par_iter()
    .map(|(title, (id, slug))| {
      let output_path = output_dir.join(slug);
      let mut writer = BufWriter::new(std::fs::File::create(output_path)?);

      let mut page = Page {
        id: *id,
        title: title.clone(),
        graph: &graph,
        included_pages: &included_pages,
        writer: &mut writer,
        highlighter,
      };

      page.render()?;

      drop(page);
      writer.flush()?;

      Ok((title.clone(), slug.clone()))
    })
    .collect::<Result<Vec<_>>>();

  pages
}
