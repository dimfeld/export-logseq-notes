use std::{collections::BTreeMap, convert::TryFrom, mem, path::PathBuf, str::FromStr};

use ahash::{HashMap, HashMapExt};
use edn_rs::{Edn, EdnError};
use eyre::Result;
use smallvec::SmallVec;

use crate::{
    content::BlockContent,
    graph::{Block, BlockInclude, ParsedPage, ViewType},
    parse_string::ContentStyle,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RoamViewType {
    Bullet,
    Numbered,
    Document,
}

impl Default for RoamViewType {
    fn default() -> RoamViewType {
        RoamViewType::Bullet
    }
}

impl TryFrom<&str> for RoamViewType {
    type Error = EdnError;

    fn try_from(val: &str) -> Result<RoamViewType, EdnError> {
        match val {
            ":bullet" => Ok(RoamViewType::Bullet),
            ":numbered" => Ok(RoamViewType::Numbered),
            ":document" => Ok(RoamViewType::Document),
            _ => Err(EdnError::ParseEdn(format!(
                "Unknown :children/view-type value {}",
                val
            ))),
        }
    }
}

#[derive(Debug, Default)]
struct RoamBlock {
    pub id: usize,
    pub title: Option<String>,
    pub string: String,
    pub uid: String,
    pub heading: usize,
    pub view_type: RoamViewType,
    pub parents: SmallVec<[usize; 1]>,
    pub children: SmallVec<[usize; 2]>,
    pub open: bool,
    pub page: usize,
    pub order: usize,
    pub refs: SmallVec<[usize; 4]>,
    pub referenced_attrs: HashMap<String, SmallVec<[AttrValue; 4]>>,

    /** Nonzero indicates that this is a daily log page (I think) */
    pub log_id: usize,

    /** An index into the graph's `emails` vector */
    pub create_email: usize,
    pub create_time: u64,
    /** An index into the graph's `emails` vector */
    pub edit_email: usize,
    pub edit_time: u64,
}

#[derive(Debug)]
pub enum AttrValue {
    Nil,
    /// A reference to another page
    Uid(String),
    /// Just a plain string
    Str(String),
}

struct EntityAttr {
    pub uid: String,
    pub value: AttrValue,
}

fn parse_attr_value(e: Edn) -> Result<AttrValue, EdnError> {
    match e {
        Edn::Nil => Ok(AttrValue::Nil),
        Edn::Str(s) => Ok(AttrValue::Str(s.trim().to_string())),
        Edn::Vector(v) => {
            let mut v = v.to_vec();
            let attr_value = v.pop();
            let attr_type = v.pop();

            match (attr_type, attr_value) {
                (Some(Edn::Key(k)), Some(Edn::Str(s))) => match k.as_str() {
                    ":block/uid" => Ok(AttrValue::Uid(s.trim().to_string())),
                    _ => Err(EdnError::ParseEdn(format!(
                        "Unknown attribute value type {}",
                        k
                    ))),
                },
                (k, v) => Err(EdnError::ParseEdn(format!(
                    "Unexpected attribute format [{:?}, {:?}]",
                    k, v
                ))),
            }
        }
        _ => Err(EdnError::ParseEdn(format!(
            "Unexpected attribute format {:?}",
            e
        ))),
    }
}

impl TryFrom<Edn> for EntityAttr {
    type Error = EdnError;

    /** Parse a value from an `:entity/attr` set. */
    fn try_from(e: Edn) -> Result<EntityAttr, EdnError> {
        /* Vector[
          {:source current-page-uid, :value current-page-uid],
          [:source referencing-block-uid, :value attr-block-uid]
          [:source referencing-block-uid, :value attr-value]
        ]

        attr value can either be a string or a uid

        uid references are all vectors of the form [":block/uid" "the-uid"]
        */

        match e {
            Edn::Vector(v) => {
                let mut values = v.to_vec();

                let m_value = values
                    .pop()
                    .ok_or_else(|| EdnError::ParseEdn("Missing attribute value".to_string()))?;
                let m_uid = values
                    .pop()
                    .ok_or_else(|| EdnError::ParseEdn("Missing attribute uid".to_string()))?;

                // Walk through the value and uid map/vectors in parallel
                match (m_uid, m_value) {
                    (Edn::Map(m_uid), Edn::Map(m_value)) => {
                        let uid = m_uid
                            .to_map()
                            .remove(":value")
                            .ok_or_else(|| {
                                EdnError::ParseEdn("No value found for attribute uid".to_string())
                            })
                            .and_then(parse_attr_value)?;

                        let value = m_value
                            .to_map()
                            .remove(":value")
                            .ok_or_else(|| {
                                EdnError::ParseEdn("No value found for attribute value".to_string())
                            })
                            .and_then(parse_attr_value)?;

                        match uid {
                            AttrValue::Uid(u) => Ok(EntityAttr { uid: u, value }),
                            // We see this sometimes. I'm not quite sure how to handle it.
                            AttrValue::Nil => Ok(EntityAttr {
                                uid: String::new(),
                                value,
                            }),
                            u => Err(EdnError::ParseEdn(format!(
                                "Unexpected attribute reference {:?}",
                                u
                            ))),
                        }
                    }
                    (uid, value) => Err(EdnError::ParseEdn(format!(
                        "Unexpected attribute values [{:?}, {:?}]",
                        uid, value
                    ))),
                }
            }
            _ => Err(EdnError::ParseEdn(format!(
                "Expected attr to be a vector, saw {:?}",
                e
            ))),
        }
    }
}

struct RoamGraph {
    pub blocks: BTreeMap<usize, RoamBlock>,
    pub blocks_by_uid: BTreeMap<String, usize>,
    pub emails: Vec<String>,
}

impl RoamGraph {
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

    fn add_block(&mut self, block: RoamBlock) {
        self.blocks_by_uid.insert(block.uid.clone(), block.id);
        self.blocks.insert(block.id, block);
    }

    fn fix_and_get_block_create_time(&mut self, block_id: usize) -> u64 {
        let block = self.blocks.get(&block_id).unwrap();
        if block.create_time > 0 {
            return block.create_time;
        }

        let mut min_create_time = u64::max_value();
        let children = block.children.clone();
        for block_id in children {
            let child_create_time = self.fix_and_get_block_create_time(block_id);
            min_create_time = min_create_time.min(child_create_time);
        }

        let block = self.blocks.get_mut(&block_id).unwrap();
        block.create_time = min_create_time;

        block.create_time
    }

    fn fix_create_times(&mut self) {
        let blocks_without_create_time = self
            .blocks
            .iter()
            .filter(|(_, b)| b.create_time == 0)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in blocks_without_create_time {
            self.fix_and_get_block_create_time(id);
        }
    }

    pub fn from_edn(mut s: &str) -> Result<RoamGraph, EdnError> {
        let mut graph = RoamGraph {
            blocks: BTreeMap::new(),
            blocks_by_uid: BTreeMap::new(),
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

        let edn = Edn::from_str(&processed)?;
        let datoms = match edn.get(":datoms") {
            Some(Edn::Vector(vec)) => vec.clone().to_vec(),
            None => return Err(EdnError::ParseEdn(String::from(":datoms was not found"))),
            _ => return Err(EdnError::ParseEdn(String::from(":datoms was not a vector"))),
        };

        let mut current_block: RoamBlock = Default::default();

        for datom_edn in datoms {
            let mut datom = match datom_edn {
                Edn::Vector(vec) => vec.to_vec(),
                _ => {
                    return Err(EdnError::ParseEdn(String::from(
                        ":datoms contains non-vector",
                    )))
                }
            };

            let value = datom.remove(2);

            let entity = datom[0].to_uint().unwrap();
            if entity != current_block.id {
                // This assumes that all attributes for a block are contiguous in the data,
                // which so far is always true.
                let mut adding_block = mem::take(&mut current_block);
                if adding_block.page == 0 {
                    adding_block.page = adding_block.id;
                }
                graph.add_block(adding_block);
            }

            let attr_item = &datom[1];

            current_block.id = entity;

            let attr = match attr_item {
                Edn::Key(attr) => attr,
                _ => {
                    return Err(EdnError::ParseEdn(format!(
                        "attr {:?} should be a key",
                        attr_item
                    )))
                }
            };

            match (attr.as_str(), value) {
                (":node/title", Edn::Str(v)) => current_block.title = Some(v),
                (":block/string", Edn::Str(v)) => current_block.string = v,
                (":block/uid", Edn::Str(v)) => current_block.uid = v,
                (":block/heading", value) => current_block.heading = value.to_uint().unwrap(),
                (":children/view-type", Edn::Key(v)) => {
                    current_block.view_type = RoamViewType::try_from(v.as_str())?
                }
                (":block/children", value) => current_block.children.push(value.to_uint().unwrap()),
                (":block/parents", value) => current_block.parents.push(value.to_uint().unwrap()),
                (":block/page", value) => current_block.page = value.to_uint().unwrap(),
                (":block/open", value) => current_block.open = value.to_bool().unwrap_or(true),
                (":block/order", value) => current_block.order = value.to_uint().unwrap(),
                (":block/refs", value) => current_block.refs.push(value.to_uint().unwrap()),
                (":log/id", value) => current_block.log_id = value.to_uint().unwrap(),

                (":create/email", Edn::Str(v)) => {
                    current_block.create_email = graph.get_email_index(v)
                }
                (":edit/email", Edn::Str(v)) => current_block.edit_email = graph.get_email_index(v),
                (":create/time", value) => {
                    current_block.create_time = value.to_uint().unwrap() as u64
                }
                (":edit/time", value) => current_block.edit_time = value.to_uint().unwrap() as u64,
                (":entity/attrs", Edn::Set(attrs)) => {
                    // List of attributes referenced within a page

                    let mut grouped: HashMap<String, SmallVec<[AttrValue; 4]>> = HashMap::default();
                    let attr_values = attrs
                        .to_set()
                        .into_iter()
                        .map(|a| EntityAttr::try_from(a).map(|ea| (ea.uid, ea.value)));

                    for attr_result in attr_values {
                        let (uid, value) = attr_result?;
                        if let AttrValue::Nil = value {
                            continue;
                        }

                        grouped.entry(uid).or_default().push(value);
                    }

                    current_block.referenced_attrs = grouped;
                }
                // Just ignore other attributes for now
                // ":attrs/lookup"
                // ":window/id"
                // ":window/filters" // Filters enabled on the page

                // These show up on special entities that only define users in the graph
                // ":user/color"
                // ":user/email"
                // ":user/settings"
                // ":user/uid"
                // ":user/display-name"
                _ => {}
            }
        }

        if current_block.page == 0 {
            current_block.page = current_block.id;
        }
        graph.add_block(current_block);

        graph.fix_create_times();

        Ok(graph)
    }
}

pub fn graph_from_roam_edn(path: &str) -> Result<(ContentStyle, bool, Vec<ParsedPage>)> {
    let roam_graph = RoamGraph::from_edn(path)?;

    let mut blocks = Vec::with_capacity(roam_graph.blocks.len());

    for roam_block in roam_graph.blocks.values() {
        let tags = roam_block
            .refs
            .iter()
            .filter_map(|attr| roam_graph.blocks.get(attr))
            .filter_map(|block| block.title.clone())
            .collect::<_>();

        let attrs = roam_block
            .referenced_attrs
            .iter()
            .map(|(tag, values)| {
                let values = values
                    .into_iter()
                    .filter_map(|value| match value {
                        AttrValue::Nil => None,
                        AttrValue::Str(s) => Some(s.clone()),
                        AttrValue::Uid(u) => roam_graph
                            .blocks_by_uid
                            .get(u.as_str())
                            .and_then(|id| roam_graph.blocks.get(id))
                            .and_then(|block| block.title.clone()),
                    })
                    .collect::<SmallVec<[String; 1]>>();

                (tag.clone(), values)
            })
            .collect::<_>();

        let view_type = match roam_block.view_type {
            RoamViewType::Bullet => ViewType::Bullet,
            RoamViewType::Numbered => ViewType::Numbered,
            RoamViewType::Document => ViewType::Document,
        };

        let block = Block {
            id: roam_block.id,
            uid: roam_block.uid.clone(),
            include_type: BlockInclude::default(),
            containing_page: roam_block.page,
            page_title: roam_block.title.clone(),
            original_title: None,
            tags,
            attrs,
            create_time: roam_block.create_time,
            edit_time: roam_block.edit_time,
            is_journal: roam_block.log_id > 0,
            extra_classes: Vec::new(),
            content_element: None,
            wrapper_element: None,

            order: roam_block.order,
            parent: roam_block.parents.first().copied(),
            children: roam_block.children.clone(),
            contents: BlockContent::new_parsed(ContentStyle::Roam, roam_block.string.clone())?,
            heading: roam_block.heading,
            view_type,
            this_block_list_type: crate::graph::ListType::Default,
        };

        blocks.push(block);
    }

    let mut pages: HashMap<usize, ParsedPage> = HashMap::new();

    for block in blocks {
        let p = pages
            .entry(block.containing_page)
            .or_insert_with(|| ParsedPage {
                path: PathBuf::from(path),
                root_block: block.containing_page,
                blocks: HashMap::default(),
            });

        p.blocks.insert(block.id, block);
    }

    let page_list = pages.into_iter().map(|(_, v)| v).collect::<Vec<_>>();

    Ok((ContentStyle::Roam, true, page_list))
}
