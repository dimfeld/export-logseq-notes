use edn_rs::{Edn, EdnError};
use fxhash::FxHashMap;
use smallvec::SmallVec;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::mem;
use std::str::FromStr;

#[derive(Debug, Copy, Clone)]
pub enum ViewType {
  Bullet,
  Numbered,
  Document,
}

impl Default for ViewType {
  fn default() -> ViewType {
    ViewType::Bullet
  }
}

impl TryFrom<&str> for ViewType {
  type Error = EdnError;

  fn try_from(val: &str) -> Result<ViewType, EdnError> {
    match val {
      ":bullet" => Ok(ViewType::Bullet),
      ":numbered" => Ok(ViewType::Numbered),
      ":document" => Ok(ViewType::Document),
      _ => Err(EdnError::ParseEdn(format!(
        "Unknown :children/view-type value {}",
        val
      ))),
    }
  }
}

#[derive(Debug, Default)]
pub struct Block {
  pub id: usize,
  pub title: Option<String>,
  pub string: String,
  pub uid: String,
  pub heading: usize,
  pub view_type: ViewType,
  pub parents: SmallVec<[usize; 1]>,
  pub children: SmallVec<[usize; 8]>,
  pub open: bool,
  pub page: usize,
  pub order: usize,
  pub refs: SmallVec<[usize; 4]>,

  /** An index into the graph's `emails` vector */
  pub create_email: usize,
  pub create_time: usize,
  /** An index into the graph's `emails` vector */
  pub edit_email: usize,
  pub edit_time: usize,
}

pub struct Graph {
  pub blocks: BTreeMap<usize, Block>,
  pub titles: FxHashMap<String, usize>,
  pub emails: Vec<String>,
}

impl Graph {
  fn get_email_index(&mut self, email: String) -> usize {
    let index = self.emails.iter().position(|s| s == &email);
    match index {
      Some(i) => i,
      None => {
        self.emails.push(email);
        self.emails.len() - 1
      }
    }
  }

  pub fn from_edn(mut s: &str) -> Result<Graph, EdnError> {
    let mut graph = Graph {
      blocks: BTreeMap::new(),
      titles: FxHashMap::default(),
      emails: Vec::<String>::new(),
    };

    // Skip past the #datascript/DB tag since this parser throws
    // an error on it.
    s = s
      .chars()
      .position(|c| c == '{')
      .map(|pos| s.split_at(pos).1)
      .unwrap();

    // This happens on image dimensions and the parser doesn't like it
    let processed = s.replace("##NaN", "0");

    let edn = Edn::from_str(processed.as_str())?;
    let datoms = match edn.get(":datoms") {
      Some(Edn::Vector(vec)) => vec.clone().to_vec(),
      None => return Err(EdnError::ParseEdn(String::from(":datoms was not found"))),
      _ => return Err(EdnError::ParseEdn(String::from(":datoms was not a vector"))),
    };

    let mut current_block: Block = Default::default();

    for datom_edn in datoms {
      let datom = match datom_edn {
        Edn::Vector(vec) => vec.to_vec(),
        _ => {
          return Err(EdnError::ParseEdn(String::from(
            ":datoms contains non-vector",
          )))
        }
      };

      let entity = datom[0].to_uint().unwrap();
      if entity != current_block.id {
        // This assumes that all attributes for a block are contiguous in the data,
        // which so far is always true.
        let adding_block = mem::take(&mut current_block);
        if let Some(title) = &adding_block.title {
          graph.titles.insert(title.clone(), adding_block.id);
        }
        graph.blocks.insert(adding_block.id, adding_block);
      }

      current_block.id = entity;

      let attr_item = &datom[1];
      let value = &datom[2];

      let attr = match attr_item {
        Edn::Key(attr) => attr,
        _ => {
          return Err(EdnError::ParseEdn(format!(
            "attr {:?} should be a key",
            attr_item
          )))
        }
      };

      match attr.as_str() {
        ":node/title" => current_block.title = Some(value.to_string()),
        ":block/string" => current_block.string = value.to_string(),
        ":block/uid" => current_block.uid = value.to_string(),
        ":block/heading" => current_block.heading = value.to_uint().unwrap(),
        ":children/view-type" => {
          current_block.view_type = ViewType::try_from(value.to_string().as_str())?
        }
        ":block/parents" => current_block.parents.push(value.to_uint().unwrap()),
        ":block/page" => current_block.page = value.to_uint().unwrap(),
        ":block/open" => current_block.open = value.to_bool().unwrap_or(true),
        ":block/order" => current_block.order = value.to_uint().unwrap(),
        ":block/refs" => current_block.refs.push(value.to_uint().unwrap()),

        ":create/email" => current_block.create_email = graph.get_email_index(value.to_string()),
        ":edit/email" => current_block.edit_email = graph.get_email_index(value.to_string()),
        ":create/time" => current_block.create_time = value.to_uint().unwrap(),
        ":edit/time" => current_block.edit_time = value.to_uint().unwrap(),
        // Just ignore other attributes for now
        _ => {}
      }
    }

    Ok(graph)
  }

  fn block_iter<F: FnMut(&(&usize, &Block)) -> bool>(
    &self,
    filter: F,
  ) -> impl Iterator<Item = &Block> {
    self.blocks.iter().filter(filter).map(|(_, n)| n)
  }

  pub fn pages(&self) -> impl Iterator<Item = &Block> {
    self.block_iter(|(_, n)| n.title.is_some())
  }

  pub fn blocks_with_reference(&self, reference: usize) -> impl Iterator<Item = &Block> {
    self.block_iter(move |(_, n)| n.refs.iter().any(move |&r| r == reference))
  }
}
