use edn_rs::{Edn, EdnError};
use smallvec::SmallVec;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::mem;
use std::str::FromStr;

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

#[derive(Default)]
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

  /** An index into the graph's `emails` vector */
  pub create_email: usize,
  pub create_time: usize,
  /** An index into the graph's `emails` vector */
  pub edit_email: usize,
  pub edit_time: usize,
}

pub struct Graph {
  pub nodes: BTreeMap<usize, Block>,
  pub emails: Vec<String>,
}

impl Graph {
  pub fn from_edn(mut s: &str) -> Result<Graph, EdnError> {
    let mut nodes = BTreeMap::<usize, Block>::new();
    let mut emails = Vec::<String>::new();

    // Skip past the #datascript/DB tag since this parser throws
    // an error on it.
    s = s
      .chars()
      .position(|c| c == '{')
      .map(|pos| s.split_at(pos).1)
      .unwrap();

    // This happens on image dimensions and the parser doesn't like it
    let processed = s.replace("##NaN", "0");

    let mut get_email_index = |email: String| {
      let index = emails.iter().position(|s| s == &email);
      match index {
        Some(i) => i,
        None => {
          emails.push(email);
          emails.len() - 1
        }
      }
    };

    let edn = Edn::from_str(processed.as_str())?;
    let datoms = match edn.get(":datoms") {
      Some(Edn::Vector(vec)) => vec.clone().to_vec(),
      None => return Err(EdnError::ParseEdn(String::from(":datoms was not found"))),
      _ => return Err(EdnError::ParseEdn(String::from(":datoms was not a vector"))),
    };

    let mut current_node: Block = Default::default();

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
      if entity != current_node.id {
        // This assumes that all attributes for a node are together,
        // which so far is always true.
        let adding_node = mem::take(&mut current_node);
        nodes.insert(adding_node.id, adding_node);
      }

      current_node.id = entity;

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
        ":node/title" => current_node.title = Some(value.to_string()),
        ":block/string" => current_node.string = value.to_string(),
        ":block/uid" => current_node.uid = value.to_string(),
        ":block/heading" => current_node.heading = value.to_uint().unwrap(),
        ":children/view-type" => {
          current_node.view_type = ViewType::try_from(value.to_string().as_str())?
        }
        ":block/parents" => current_node.parents.push(value.to_uint().unwrap()),
        ":block/page" => current_node.page = value.to_uint().unwrap(),
        ":block/open" => current_node.open = value.to_bool().unwrap_or(true),
        ":block/order" => current_node.order = value.to_uint().unwrap(),

        ":create/email" => current_node.create_email = get_email_index(value.to_string()),
        ":edit/email" => current_node.edit_email = get_email_index(value.to_string()),
        ":create/time" => current_node.create_time = value.to_uint().unwrap(),
        ":edit/time" => current_node.edit_time = value.to_uint().unwrap(),
        // Just ignore them for now
        _ => {}
      }
    }

    Ok(Graph { nodes, emails })
  }
}
