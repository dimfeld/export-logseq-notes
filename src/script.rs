use crate::{
    graph::{AttrList, Block, BlockInclude, Graph, ViewType},
    make_pages::title_to_slug,
};

use ahash::HashMap;
use eyre::{eyre, Result};
use rhai::{def_package, packages::StandardPackage, plugin::*, Scope, AST};
use smallvec::smallvec;
use std::sync::{Arc, Mutex};

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

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub enum AllowEmbed {
    #[default]
    Default,
    Yes,
    No,
}

#[derive(Debug, Clone, Default)]
pub enum TemplateSelection {
    /// The default template defined in the config
    #[default]
    Default,
    /// The filename of a template to render
    File(String),
    /// A template value itself.
    Value(String),
}

#[derive(Debug, Clone)]
pub struct PageConfig {
    pub path_base: String,
    pub path_name: String,
    pub url_base: String,
    pub url_name: String,
    pub title: String,
    pub attrs: HashMap<String, AttrList>,
    pub tags: AttrList,
    pub is_journal: bool,
    pub template: TemplateSelection,

    pub include: bool,
    pub allow_embedding: AllowEmbed,

    pub root_block: usize,
}

#[derive(Debug, Clone)]
pub struct PageObject {
    pub config: PageConfig,
    pub graph: Arc<Mutex<Graph>>,
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
    pub include_type: BlockInclude,

    edited: bool,
}

impl BlockConfig {
    fn from_block(block: &Block) -> Self {
        BlockConfig {
            id: block.id,
            string: block.string.clone(),
            heading: block.heading,
            view_type: block.view_type,
            include_type: block.include_type,
            edited: false,
        }
    }

    fn apply_to_block(self, block: &mut Block) {
        if self.edited {
            block.string = self.string;
            block.heading = self.heading;
            block.view_type = self.view_type;
            block.include_type = self.include_type;
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
) -> Result<()> {
    let block_config = {
        let mut graph = page.graph.lock().unwrap();
        let block = graph.blocks.get_mut(&block_id).unwrap();
        BlockConfig::from_block(block)
    };

    let mut d = Dynamic::from(block_config).into_shared();
    callback.call_raw(context, Some(&mut d), [depth.into()])?;

    let block_config = d.cast::<BlockConfig>();

    let mut graph = page.graph.lock().unwrap();
    let block = graph.blocks.get_mut(&block_id).unwrap();
    block_config.apply_to_block(block);

    let next_depth = depth + 1;
    if next_depth <= max_depth {
        let children = block.children.clone();
        drop(graph);
        for child_id in children {
            walk_block(context, page, max_depth, next_depth, child_id, callback)?;
        }
    }

    Ok(())
}

#[export_module]
pub mod rhai_block {
    pub type Block = BlockConfig;

    /// Get the text contents of the block.
    #[rhai_fn(get = "contents", pure)]
    pub fn get_string(block: &mut BlockConfig) -> String {
        block.string.to_string()
    }

    /// Set the text contents of the block.
    #[rhai_fn(set = "contents", pure)]
    pub fn set_string(block: &mut BlockConfig, value: String) {
        block.string = value;
        block.edited = true;
    }

    /// Get the heading level of the block.
    #[rhai_fn(get = "heading", pure)]
    pub fn get_heading(block: &mut BlockConfig) -> i64 {
        block.heading as i64
    }

    /// Set the heading level of the block.
    #[rhai_fn(set = "heading", pure)]
    pub fn set_heading(block: &mut BlockConfig, value: i64) {
        block.heading = value as usize;
        block.edited = true;
    }

    /// Get the view type of the block
    #[rhai_fn(get = "view_type", pure)]
    pub fn get_view_type(block: &mut BlockConfig) -> ViewType {
        block.view_type
    }

    /// Set the ViewType of the block
    #[rhai_fn(set = "view_type", pure)]
    pub fn set_view_type(block: &mut BlockConfig, value: ViewType) {
        block.view_type = value;
        block.edited = true;
    }

    #[rhai_fn(get = "include", pure)]
    pub fn get_include(block: &mut BlockConfig) -> BlockInclude {
        block.include_type
    }

    #[rhai_fn(set = "include")]
    pub fn set_include(block: &mut BlockConfig, include: BlockInclude) {
        block.include_type = include;
        block.edited = true;
    }
}

#[export_module]
pub mod rhai_page {
    use rhai::FnPtr;

    pub type Page = PageObject;

    #[rhai_fn(get = "is_journal", pure)]
    pub fn is_journal(page: &mut Page) -> bool {
        page.config.is_journal
    }

    #[rhai_fn(get = "path_base", pure)]
    pub fn get_path_base(page: &mut Page) -> String {
        page.config.path_base.to_string()
    }

    #[rhai_fn(set = "path_base")]
    pub fn set_path_base(page: &mut Page, value: String) {
        page.config.path_base = value;
    }

    /// Get the filename of the rendered page file. The default value is "", which indicates
    /// that the filename will be the `url_name` plus an appropriate extension.
    #[rhai_fn(get = "path_name", pure)]
    pub fn get_path_name(page: &mut Page) -> String {
        page.config.path_name.to_string()
    }

    /// Set the filename of the rendered page file. If empty, it will be the `url_name` plus
    /// an appropriate extension.
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

    #[rhai_fn(get = "title", pure)]
    pub fn get_title(page: &mut Page) -> String {
        page.config.title.to_string()
    }

    #[rhai_fn(set = "title")]
    pub fn set_title(page: &mut Page, value: String) {
        page.config.title = value;
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
    pub fn allow_render(page: &mut Page, value: bool) {
        page.config.include = value;
    }

    #[rhai_fn(global)]
    pub fn add_tag(page: &mut Page, value: String) {
        page.config.tags.push(value);
    }

    #[rhai_fn(global)]
    pub fn remove_tag(page: &mut Page, value: String) {
        let pos = page.config.tags.iter().position(|v| v == &value);
        if let Some(index) = pos {
            page.config.tags.swap_remove(index);
        }
    }

    #[rhai_fn(set = "tags")]
    pub fn set_tags(page: &mut Page, tags: Vec<String>) {
        page.config.tags = tags.into();
    }

    #[rhai_fn(get = "tag", pure)]
    pub fn get_tags(page: &mut Page) -> Vec<String> {
        page.config.tags.iter().cloned().collect()
    }

    #[rhai_fn(global)]
    pub fn set_attr(page: &mut Page, attr: String, value: String) {
        page.config.attrs.insert(attr, smallvec![value]);
    }

    #[rhai_fn(global)]
    pub fn set_attr_values(page: &mut Page, attr: String, value: Vec<String>) {
        page.config.attrs.insert(attr, value.into());
    }

    #[rhai_fn(global)]
    pub fn get_attr_first(page: &mut Page, attr: String) -> String {
        page.config
            .attrs
            .get(&attr)
            .and_then(|v| v.get(0))
            .cloned()
            .unwrap_or_default()
    }

    #[rhai_fn(global)]
    pub fn get_attr(page: &mut Page, attr: String) -> Vec<String> {
        page.config
            .attrs
            .get(&attr)
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_else(Vec::new)
    }

    #[rhai_fn(global)]
    pub fn each_block(
        context: NativeCallContext,
        page: &mut Page,
        max_depth: i64,
        callback: FnPtr,
    ) -> std::result::Result<(), String> {
        super::walk_block(
            &context,
            page,
            max_depth,
            0,
            page.config.root_block,
            &callback,
        )
        .map_err(|e| format!("{e:?}"))
    }

    #[rhai_fn(global)]
    pub fn set_template_file(page: &mut Page, filename: String) {
        page.config.template = TemplateSelection::File(filename);
    }

    #[rhai_fn(global)]
    pub fn set_template_contents(page: &mut Page, contents: String) {
        page.config.template = TemplateSelection::Value(contents);
    }
}

create_enum!(allow_embed_module : super::AllowEmbed => Default, Yes, No);
create_enum!(block_include_module : super::BlockInclude => AndChildren, OnlyChildren, JustBlock, Exclude, IfChildrenPresent);
create_enum!(view_type_module : crate::graph::ViewType => Inherit, Bullet, Numbered, Document);

def_package! {
    pub ParsePackage(module) : StandardPackage {
        combine_with_exported_module!(module, "page", rhai_page);
        combine_with_exported_module!(module, "block", rhai_block);
    } |> |engine| {
        engine
            .register_type_with_name::<AllowEmbed>("AllowEmbed")
            .register_static_module("AllowEmbed", exported_module!(allow_embed_module).into())
            .register_type_with_name::<BlockInclude>("BlockInclude")
            .register_static_module(
                "BlockInclude",
                exported_module!(block_include_module).into(),
            )
            .register_type_with_name::<ViewType>("ViewType")
            .register_static_module("ViewType", exported_module!(view_type_module).into());
    }
}

pub fn run_script_on_page(
    engine: &mut Engine,
    ast: &AST,
    graph: &Arc<Mutex<Graph>>,
    block_id: usize,
) -> Result<PageConfig> {
    let page_config = {
        let g = graph.lock().unwrap();
        let page = g.blocks.get(&block_id).expect("Block must exist");
        let title = page.page_title.clone().expect("Page title must exist");

        let slug = title_to_slug(&title);

        PageConfig {
            include: true,
            path_base: String::new(),
            path_name: String::new(),
            url_base: String::new(),
            url_name: slug,
            title,
            template: TemplateSelection::Default,
            is_journal: page.is_journal,
            attrs: page.attrs.clone(),
            tags: page.tags.clone(),
            allow_embedding: AllowEmbed::Default,
            root_block: block_id,
        }
    };

    let page_object = PageObject {
        config: page_config,
        graph: graph.clone(),
    };

    let dy = Dynamic::from(page_object).into_shared();
    let mut scope = Scope::new();
    scope.push_dynamic("page", dy.clone());

    engine
        .run_ast_with_scope(&mut scope, ast)
        .map_err(|e| eyre!("{e:?}"))?;

    drop(scope);
    let page_object = dy.cast::<PageObject>();

    Ok(page_object.config)
}
