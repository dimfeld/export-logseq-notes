use crate::roam_edn::*;
use anyhow::{anyhow, Result};
use fxhash::FxHashMap;
use std::borrow::Cow;

pub struct Page<'a> {
  pub title: String,
  pub text: Vec<Cow<'a, str>>,
  pub slug: String,
}

fn process_text<'a>(s: &'a str) -> Cow<'a, str> {
  Cow::Borrowed(s)
}

fn gather_text<'a>(
  graph: &'a Graph,
  block_id: usize,
  included_pages: &FxHashMap<usize, String>,
  depth: usize,
  view_type: ViewType,
) -> Vec<Cow<'a, str>> {
  let block = graph.blocks.get(&block_id).unwrap();

  let mut output = Vec::with_capacity(block.children.len() + 1);

  if depth > 0 || !block.string.is_empty() {
    output.push(process_text(&block.string));
  }

  let text = block
    .children
    .iter()
    .flat_map(|&id| gather_text(graph, id, included_pages, depth + 1, block.view_type));
  output.extend(text);

  output
}

pub fn make_pages<'a>(graph: &'a Graph, filter_tag: &str) -> Result<Vec<Page<'a>>> {
  let tag_node_id = *graph
    .titles
    .get(filter_tag)
    .ok_or_else(|| anyhow!("Could not find page with filter name {}", filter_tag))?;

  let included_pages = graph
    .pages()
    .filter(|block| block.refs.iter().any(|&r| r == tag_node_id))
    // TODO Change title to slug
    .map(|block| (block.id, block.title.clone().unwrap()))
    .collect::<FxHashMap<_, _>>();

  let pages = included_pages
    .iter()
    .map(|(&id, slug)| {
      let block = graph.blocks.get(&id).unwrap();
      let text = gather_text(graph, id, &included_pages, 0, block.view_type);
      Page {
        title: block.title.clone().unwrap(),
        text,
        slug: String::from(slug),
      }
    })
    .collect::<Vec<_>>();

  Ok(pages)
}
