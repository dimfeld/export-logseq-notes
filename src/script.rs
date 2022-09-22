use std::{borrow::Cow, sync::Mutex};

use crate::graph::{Block, Graph, ViewType};

use ahash::{AHashMap, AHashSet};
use eyre::Result;
use rhai::{def_package, plugin::*, CustomType, TypeBuilder, AST};
use smallvec::SmallVec;
use std::rc::Rc;

///
/// set_path_base(path) -- Set the base output folder for this page
/// set_path_name(filename) -- Override the filename for this page. Can include directories
/// set_url_base(base_url) -- Set the base URL for this page. Mostly used when linking to it from other pages.
/// set_url_name(name) -- Override the URL path for this page. Appended to URL base, if set.
///
/// allow_render(PageInclude::All|PageInclude::None|PageInclude::Partial) -- Render the entire page
/// allow_embedding(bool) -- Allow embedding this page. Defaults to true if the page is allowed to
///     render, false if not.
///
/// tags() -- Return a list of tags
/// skip_rendering_tags(["tag"]) -- Don't render these tags if they appear in the content.
/// add_tags([tag]) -- Add this tag to the page
/// remove_tags([tag]) -- Remove this tag from the page
///
/// set_attr(name, value)
/// remove_attr(name)
///
/// each_block(max_depth, |block, depth| { }) -- Call this callback for each block in the page, up
///     to max_depth
///
/// // Include a block if allow_render is set to Partial.
/// include_block(block_id, 'AndChildren'|'OnlyChildren'|'JustBlock')
///
/// exclude_block(block_id) -- If rendering this page, exclude this block and its children.

#[derive(Debug, Clone, Default)]
pub enum PageInclude {
    /// Render only pages included via include_block
    #[default]
    Partial,
    /// Render the entire page
    All,
    /// Omit the entire page
    Omit,
}

#[derive(Debug, Copy, Clone)]
pub enum BlockInclude {
    AndChildren,
    OnlyChlidren,
    JustBlock,
}

#[derive(Debug, Copy, Clone, Default)]
pub enum AllowEmbed {
    #[default]
    Default,
    Yes,
    No,
}

#[derive(Debug, Clone)]
pub struct PageConfig {
    pub path_base: String,
    pub path_name: String,
    pub url_base: String,
    pub url_name: String,
    pub attrs: AHashMap<String, String>,
    pub tags: AHashSet<String>,

    pub include: PageInclude,
    pub include_blocks: SmallVec<[(usize, BlockInclude); 4]>,
    pub exclude_blocks: SmallVec<[usize; 4]>,

    pub allow_embedding: AllowEmbed,

    pub root_block: usize,
}

#[derive(Debug, Clone)]
pub struct PageObject {
    pub config: PageConfig,
    // Wrap the graph in an Rc since Rhai objects need to be Clonable.
    pub graph: Rc<Mutex<Graph>>,
}

macro_rules! create_enum {
    ($module:ident : $typ:ty => $($variant:ident),+) => {
        #[export_module]
        pub mod $module {
            $(
                #[allow(non_upper_case_globals)]
                pub const $variant: $typ = <$typ>::$variant;
            )*
        }
    };
}

#[derive(Debug, Clone)]
pub struct BlockConfig {
    pub id: usize,
    pub string: String,
    pub heading: usize,
    pub view_type: ViewType,

    edited: bool,
}

impl BlockConfig {
    fn from_block(block: &Block) -> Self {
        BlockConfig {
            id: block.id,
            string: block.string.clone(),
            heading: block.heading,
            view_type: block.view_type,
            edited: false,
        }
    }

    fn apply_to_block(self, block: &mut Block) {
        if self.edited {
            block.string = self.string;
            block.heading = self.heading;
            block.view_type = self.view_type;
        }
    }
}

fn walk_block(
    context: &NativeCallContext,
    page: &mut PageObject,
    max_depth: i64,
    depth: i64,
    block_id: usize,
    callback: &rhai::FnPtr,
) {
    let mut graph = page.graph.lock().expect("acquiring graph mutex");
    let block = graph.blocks.get_mut(&block_id).unwrap();
    let block_config = BlockConfig::from_block(block);

    let mut d = Dynamic::from(block_config);
    callback
        .call_raw(context, Some(&mut d), [depth.into()])
        .expect("calling block callback");

    let block_config = d.cast::<BlockConfig>();
    block_config.apply_to_block(block);

    let next_depth = depth + 1;
    if next_depth < max_depth {
        let children = block.children.clone();
        drop(graph);
        for child_id in children {
            walk_block(context, page, max_depth, next_depth, child_id, callback);
        }
    }
}

#[export_module]
pub mod rhai_block {
    pub type Block = BlockConfig;

    #[rhai_fn(get = "string", pure)]
    pub fn get_string(block: &mut BlockConfig) -> String {
        block.string.to_string()
    }

    #[rhai_fn(set = "string", pure)]
    pub fn set_string(block: &mut BlockConfig, value: String) {
        block.string = value;
        block.edited = true;
    }

    #[rhai_fn(get = "heading", pure)]
    pub fn get_heading(block: &mut BlockConfig) -> i64 {
        block.heading as i64
    }

    #[rhai_fn(set = "heading", pure)]
    pub fn set_heading(block: &mut BlockConfig, value: i64) {
        block.heading = value as usize;
        block.edited = true;
    }

    #[rhai_fn(get = "view_type", pure)]
    pub fn get_view_type(block: &mut BlockConfig) -> ViewType {
        block.view_type
    }

    #[rhai_fn(set = "view_type", pure)]
    pub fn set_view_type(block: &mut BlockConfig, value: ViewType) {
        block.view_type = value;
        block.edited = true;
    }
}

#[export_module]
pub mod rhai_page {
    use rhai::FnPtr;

    pub type Page = PageObject;

    #[rhai_fn(get = "path_base", pure)]
    pub fn get_path_base(page: &mut Page) -> String {
        page.config.path_base.to_string()
    }

    #[rhai_fn(set = "path_base")]
    pub fn set_path_base(page: &mut Page, value: String) {
        page.config.path_base = value;
    }

    #[rhai_fn(get = "path_name", pure)]
    pub fn get_path_name(page: &mut Page) -> String {
        page.config.path_name.to_string()
    }

    #[rhai_fn(set = "path_name")]
    pub fn set_path_name(page: &mut Page, value: String) {
        page.config.path_name = value;
    }

    #[rhai_fn(get = "url_base", pure)]
    pub fn get_url_base(page: &mut Page) -> String {
        page.config.url_base.to_string()
    }

    #[rhai_fn(set = "url_base")]
    pub fn set_url_base(page: &mut Page, value: String) {
        page.config.url_base = value;
    }

    #[rhai_fn(get = "url_name", pure)]
    pub fn get_url_name(page: &mut Page) -> String {
        page.config.url_name.to_string()
    }

    #[rhai_fn(set = "url_name")]
    pub fn set_url_name(page: &mut Page, value: String) {
        page.config.url_name = value;
    }

    #[rhai_fn(get = "allow_embedding")]
    pub fn get_allow_embedding(page: &mut Page) -> AllowEmbed {
        page.config.allow_embedding
    }

    #[rhai_fn(set = "allow_embedding")]
    pub fn set_allow_embedding(page: &mut Page, value: AllowEmbed) {
        page.config.allow_embedding = value
    }

    #[rhai_fn(global)]
    pub fn allow_render(page: &mut Page, value: PageInclude) {
        page.config.include = value;
    }

    #[rhai_fn(global)]
    pub fn each_block(
        context: NativeCallContext,
        page: &mut Page,
        max_depth: i64,
        callback: FnPtr,
    ) {
        super::walk_block(
            &context,
            page,
            max_depth,
            0,
            page.config.root_block,
            &callback,
        );
    }

    #[rhai_fn(global)]
    pub fn include_block(page: &mut Page, block_id: i64, include: BlockInclude) {
        page.config
            .include_blocks
            .push((block_id as usize, include));
    }

    #[rhai_fn(global)]
    pub fn exclude_block(page: &mut Page, block_id: i64) {
        page.config.exclude_blocks.push(block_id as usize)
    }
}

create_enum!(allow_embed_module : super::AllowEmbed => Default, Yes, No);
create_enum!(block_include_module : super::BlockInclude => AndChildren, OnlyChlidren, JustBlock);
create_enum!(page_include_module : super::PageInclude => Partial, All, Omit);
create_enum!(view_type_module : crate::graph::ViewType => Bullet, Numbered, Document);

pub fn create_engine() -> Engine {
    let mut engine = Engine::new();

    engine
        .register_global_module(exported_module!(rhai_page).into())
        .register_global_module(exported_module!(rhai_block).into())
        .register_type_with_name::<AllowEmbed>("AllowEmbed")
        .register_static_module("AllowEmbed", exported_module!(allow_embed_module).into())
        .register_type_with_name::<BlockInclude>("BlockInclude")
        .register_static_module(
            "BlockInclude",
            exported_module!(block_include_module).into(),
        )
        .register_type_with_name::<PageInclude>("PageInclude")
        .register_static_module("PageInclude", exported_module!(page_include_module).into())
        .register_type_with_name::<ViewType>("ViewType")
        .register_static_module("ViewType", exported_module!(view_type_module).into());

    engine
}

pub fn run_script_on_page(
    engine: &mut Engine,
    ast: &AST,
    graph: Rc<Mutex<Graph>>,
    block_id: usize,
) -> Result<PageConfig> {
    let g = graph.lock().expect("acquiring graph mutex");

    todo!()
}
