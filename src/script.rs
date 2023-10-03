use std::sync::{Arc, Mutex};

use ahash::{HashMap, HashSet};
use eyre::{eyre, Result};
use regex::RegexSet;
use rhai::{
    def_package,
    packages::{Package, StandardPackage},
    plugin::*,
    Scope, AST,
};
use smallvec::smallvec;

use crate::{
    config::Config,
    content::BlockContent,
    graph::{AttrList, Block, BlockInclude, ParsedPage, ViewType},
    make_pages::title_to_slug,
};

type SmartString = smartstring::SmartString<smartstring::LazyCompact>;

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
    pub picture_template: TemplateSelection,
    pub picture_upload_profile: Option<String>,

    pub include: bool,
    pub allow_embedding: AllowEmbed,
    pub top_header_level: usize,

    pub root_block: usize,
}

#[derive(Debug, Clone)]
pub struct PageObject {
    pub config: PageConfig,
    pub page: Arc<Mutex<ParsedPage>>,
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
    pub tags: AttrList,
    pub attrs: HashMap<String, AttrList>,

    pub content_element: Option<String>,
    pub wrapper_element: Option<String>,
    pub extra_classes: Vec<String>,

    edited: bool,
}

impl BlockConfig {
    fn from_block(block: &Block) -> Self {
        BlockConfig {
            id: block.id,
            string: block.contents.borrow_string().clone(),
            heading: block.heading,
            view_type: block.view_type,
            include_type: block.include_type,
            tags: block.tags.clone(),
            attrs: block.attrs.clone(),
            content_element: block.content_element.clone(),
            wrapper_element: block.wrapper_element.clone(),
            extra_classes: block.extra_classes.clone(),
            edited: false,
        }
    }

    fn apply_to_block(self, block: &mut Block) -> Result<()> {
        if self.edited {
            block.contents = BlockContent::new_parsed(*block.contents.borrow_style(), self.string)?;
            block.heading = self.heading;
            block.view_type = self.view_type;
            block.include_type = self.include_type;
            block.tags = self.tags;
            block.attrs = self.attrs;
            block.content_element = self.content_element;
            block.wrapper_element = self.wrapper_element;
            block.extra_classes = self.extra_classes;
        }

        Ok(())
    }
}

#[export_module]
pub mod rhai_block {
    pub type Block = BlockConfig;

    #[rhai_fn(get = "id", pure)]
    pub fn get_id(block: &mut BlockConfig) -> i64 {
        block.id as i64
    }

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

    #[rhai_fn(get = "tags", pure)]
    pub fn get_tags(block: &mut BlockConfig) -> Vec<Dynamic> {
        block
            .tags
            .iter()
            .map(|s| Dynamic::from(s.to_string()))
            .collect()
    }

    #[rhai_fn(set = "tags")]
    pub fn set_tags(block: &mut BlockConfig, tags: Vec<String>) {
        block.tags = tags.into();
        block.edited = true;
    }

    #[rhai_fn(get = "content_element", pure)]
    pub fn get_content_element(block: &mut BlockConfig) -> Option<String> {
        block.content_element.clone()
    }

    #[rhai_fn(set = "content_element")]
    pub fn set_content_element(block: &mut BlockConfig, element: String) {
        if element.is_empty() {
            block.content_element = None;
        } else {
            block.content_element = Some(element);
        }
        block.edited = true;
    }

    #[rhai_fn(get = "wrapper_element", pure)]
    pub fn get_wrapper_element(block: &mut BlockConfig) -> Option<String> {
        block.wrapper_element.clone()
    }

    #[rhai_fn(set = "wrapper_element")]
    pub fn set_wrapper_element(block: &mut BlockConfig, element: String) {
        if element.is_empty() {
            block.wrapper_element = None;
        } else {
            block.wrapper_element = Some(element);
        }
        block.edited = true;
    }

    #[rhai_fn(get = "classlist", pure)]
    pub fn get_class(block: &mut BlockConfig) -> Vec<Dynamic> {
        block
            .extra_classes
            .iter()
            .map(|s| Dynamic::from(s.to_string()))
            .collect()
    }

    #[rhai_fn(set = "classlist")]
    pub fn set_class(block: &mut BlockConfig, class: Vec<Dynamic>) {
        block.extra_classes = class
            .into_iter()
            .filter_map(|s| s.into_string().ok())
            .collect::<Vec<String>>();
        block.edited = true;
    }

    #[rhai_fn(global)]
    pub fn get_attr_first(block: &mut Block, attr: String) -> String {
        block
            .attrs
            .get(&attr)
            .and_then(|v| v.get(0))
            .cloned()
            .unwrap_or_default()
    }

    #[rhai_fn(global)]
    pub fn get_attr(block: &mut Block, attr: String) -> Vec<Dynamic> {
        block
            .attrs
            .get(&attr)
            .map(|l| l.iter().map(|s| Dynamic::from(s.to_string())).collect())
            .unwrap_or_else(Vec::new)
    }

    #[rhai_fn(global)]
    pub fn remove_attr(block: &mut Block, name: String) {
        let removed = block.attrs.remove(&name).is_some();
        if removed {
            block.edited = true;
        }
    }
}

#[export_module]
pub mod rhai_page {
    use rhai::FnPtr;

    pub type Page = PageConfig;

    #[rhai_fn(global)]
    pub fn to_slug(s: String) -> String {
        title_to_slug(&s)
    }

    #[rhai_fn(get = "is_journal", pure)]
    pub fn is_journal(page: &mut Page) -> bool {
        page.is_journal
    }

    #[rhai_fn(get = "root_block", pure)]
    pub fn root_block(page: &mut Page) -> i64 {
        page.root_block as i64
    }

    #[rhai_fn(get = "path_base", pure)]
    pub fn get_path_base(page: &mut Page) -> String {
        page.path_base.to_string()
    }

    #[rhai_fn(set = "path_base")]
    pub fn set_path_base(page: &mut Page, value: String) {
        page.path_base = value;
    }

    /// Get the filename of the rendered page file. The default value is "", which indicates
    /// that the filename will be the `url_name` plus an appropriate extension.
    #[rhai_fn(get = "path_name", pure)]
    pub fn get_path_name(page: &mut Page) -> String {
        page.path_name.to_string()
    }

    /// Set the filename of the rendered page file. If empty, it will be the `url_name` plus
    /// an appropriate extension.
    #[rhai_fn(set = "path_name")]
    pub fn set_path_name(page: &mut Page, value: String) {
        page.path_name = value;
    }

    #[rhai_fn(get = "url_base", pure)]
    pub fn get_url_base(page: &mut Page) -> String {
        page.url_base.to_string()
    }

    #[rhai_fn(set = "url_base")]
    pub fn set_url_base(page: &mut Page, value: String) {
        page.url_base = value;
    }

    #[rhai_fn(get = "url_name", pure)]
    pub fn get_url_name(page: &mut Page) -> String {
        page.url_name.to_string()
    }

    #[rhai_fn(set = "url_name")]
    pub fn set_url_name(page: &mut Page, value: String) {
        page.url_name = value;
    }

    #[rhai_fn(get = "title", pure)]
    pub fn get_title(page: &mut Page) -> String {
        page.title.to_string()
    }

    #[rhai_fn(set = "title")]
    pub fn set_title(page: &mut Page, value: String) {
        page.title = value;
    }

    #[rhai_fn(get = "allow_embedding")]
    pub fn get_allow_embedding(page: &mut Page) -> AllowEmbed {
        page.allow_embedding
    }

    #[rhai_fn(set = "allow_embedding")]
    pub fn set_allow_embedding(page: &mut Page, value: AllowEmbed) {
        page.allow_embedding = value
    }

    #[rhai_fn(get = "include", pure)]
    pub fn get_include(page: &mut Page) -> bool {
        page.include
    }

    #[rhai_fn(set = "include")]
    pub fn set_include(page: &mut Page, value: bool) {
        page.include = value;
    }

    #[rhai_fn(get = "top_header_level", pure)]
    pub fn get_top_header_level(page: &mut Page) -> usize {
        page.top_header_level
    }

    #[rhai_fn(set = "top_header_level")]
    pub fn set_top_header_level(page: &mut Page, value: i64) {
        page.top_header_level = value as usize;
    }

    #[rhai_fn(global)]
    pub fn add_tag(page: &mut Page, value: String) {
        if !page.tags.contains(&value) {
            page.tags.push(value);
        }
    }

    #[rhai_fn(global)]
    pub fn add_tags(page: &mut Page, values: Vec<Dynamic>) {
        for value in values {
            if let Ok(v) = value.into_string() {
                if !page.tags.contains(&v) {
                    page.tags.push(v);
                }
            }
        }
    }

    #[rhai_fn(global)]
    pub fn remove_tag(page: &mut Page, value: String) {
        page.tags.retain(|v| v != &value);
    }

    #[rhai_fn(set = "tags")]
    pub fn set_tags(page: &mut Page, tags: Vec<Dynamic>) {
        page.tags = tags
            .into_iter()
            .filter_map(|t| t.into_string().ok())
            .collect();
    }

    #[rhai_fn(get = "tags", pure)]
    pub fn get_tags(page: &mut Page) -> Vec<Dynamic> {
        page.tags
            .iter()
            .map(|s| Dynamic::from(s.to_string()))
            .collect()
    }

    #[rhai_fn(global)]
    pub fn remove_attr(page: &mut Page, name: String) {
        page.attrs.remove(&name);
    }

    #[rhai_fn(global)]
    pub fn set_attr(page: &mut Page, attr: String, value: String) {
        page.attrs.insert(attr, smallvec![value]);
    }

    #[rhai_fn(global)]
    pub fn set_attr_values(page: &mut Page, attr: String, value: Vec<String>) {
        page.attrs.insert(attr, value.into());
    }

    #[rhai_fn(global)]
    pub fn get_attr_first(page: &mut Page, attr: String) -> String {
        page.attrs
            .get(&attr)
            .and_then(|v| v.get(0))
            .cloned()
            .unwrap_or_default()
    }

    #[rhai_fn(global)]
    pub fn get_attr(page: &mut Page, attr: String) -> Vec<Dynamic> {
        page.attrs
            .get(&attr)
            .map(|l| l.iter().map(|s| Dynamic::from(s.to_string())).collect())
            .unwrap_or_else(Vec::new)
    }

    #[rhai_fn(global)]
    pub fn set_template_file(page: &mut Page, filename: String) {
        page.template = TemplateSelection::File(filename);
    }

    #[rhai_fn(global)]
    pub fn set_template_contents(page: &mut Page, contents: String) {
        page.template = TemplateSelection::Value(contents);
    }

    #[rhai_fn(global)]
    pub fn set_picture_template_file(page: &mut Page, filename: String) {
        page.picture_template = TemplateSelection::File(filename);
    }

    #[rhai_fn(global)]
    pub fn set_picture_template_contents(page: &mut Page, contents: String) {
        page.picture_template = TemplateSelection::Value(contents);
    }

    #[rhai_fn(global)]
    pub fn set_picture_upload_profile(page: &mut Page, profile: String) {
        page.picture_upload_profile = Some(profile);
    }
}

pub fn each_block(
    context: NativeCallContext,
    page: &Arc<Mutex<ParsedPage>>,
    max_depth: i64,
    callback: rhai::FnPtr,
) -> Result<(), Box<EvalAltResult>> {
    let root_block = {
        let p = page.lock().unwrap();
        p.root_block
    };

    walk_block(&context, page, max_depth, 0, root_block, &callback)
}

fn walk_block(
    context: &NativeCallContext,
    page: &Arc<Mutex<ParsedPage>>,
    max_depth: i64,
    depth: i64,
    block_id: usize,
    callback: &rhai::FnPtr,
) -> Result<(), Box<EvalAltResult>> {
    let block_config = {
        let p = page.lock().unwrap();
        let block = p.blocks.get(&block_id).unwrap();
        BlockConfig::from_block(block)
    };

    let d = Dynamic::from(block_config).into_shared();
    let output: Dynamic =
        callback.call_within_context(context, (d.clone(), Dynamic::from(depth)))?;

    let block_config = match output.try_cast::<BlockConfig>() {
        Some(b) => b,
        None => d.cast::<BlockConfig>(),
    };

    let mut p = page.lock().unwrap();
    let block = p.blocks.get_mut(&block_id).unwrap();
    block_config.apply_to_block(block).map_err(|e| {
        Box::new(EvalAltResult::ErrorSystem(
            String::from("Invalid block markdown"),
            e.into(),
        ))
    })?;

    let next_depth = depth + 1;
    if next_depth <= max_depth {
        let children = block.children.clone();
        drop(p);
        for child_id in children {
            walk_block(context, page, max_depth, next_depth, child_id, callback)?;
        }
    }

    Ok(())
}

/// Scan the entire contents of a page, and perform case-insensitive matching to generate tags.
/// For each key and value pair, the key is the searched-for word string and the value is the
/// tag assigned if it is found.
pub fn autotag(
    page: &Arc<Mutex<ParsedPage>>,
    input: rhai::Map,
    start_block: i64,
) -> Result<Vec<Dynamic>, Box<EvalAltResult>> {
    let num_searches = input.len();
    let (searches, tags) = input
        .into_iter()
        .map(|(search, tag)| {
            let tag = tag.into_immutable_string()?;

            let search = format!(
                r##"(?:^|\b){s}(?:\b|$)"##,
                s = regex::escape(search.as_str())
            );

            Ok::<_, String>((search, tag))
        })
        .try_fold(
            (
                Vec::with_capacity(num_searches),
                Vec::with_capacity(num_searches),
            ),
            |mut acc, x| {
                let x = x?;
                acc.0.push(x.0);
                acc.1.push(x.1);
                Ok::<_, Box<EvalAltResult>>(acc)
            },
        )?;

    let re = regex::RegexSetBuilder::new(&searches)
        .case_insensitive(true)
        .build()
        .map_err(|e| e.to_string())?;

    let mut results = HashSet::default();
    autotag_block_and_children(page, &mut results, start_block as usize, &re, &tags)?;

    let results = results
        .into_iter()
        .map(Dynamic::from)
        .collect::<Vec<Dynamic>>();

    Ok(results)
}

fn autotag_block_and_children(
    page: &Arc<Mutex<ParsedPage>>,
    results: &mut HashSet<String>,
    block_id: usize,
    tags: &RegexSet,
    matches: &[rhai::ImmutableString],
) -> Result<(), Box<EvalAltResult>> {
    let children = {
        let p = page.lock().unwrap();
        let block = p
            .blocks
            .get(&block_id)
            .ok_or_else(|| format!("Could not find block {block_id}"))?;

        for m in tags.matches(block.contents.borrow_string()) {
            let tag = &matches[m];
            results.insert(tag.clone().into_owned());
        }

        block.children.clone()
    };

    for child in children {
        autotag_block_and_children(page, results, child, tags, matches)?;
    }

    Ok(())
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
    package: &ParsePackage,
    ast: &AST,
    global_config: &Config,
    page: ParsedPage,
) -> Result<(PageConfig, ParsedPage)> {
    let mut engine = Engine::new_raw();

    engine.on_print(|x| println!("script: {x}"));
    engine.on_debug(|x, _src, pos| {
        println!("script:{pos:?}: {x}");
    });

    package.register_into_engine(&mut engine);

    let page_block = page.blocks.get(&page.root_block).expect("Block must exist");
    let title = page_block
        .page_title
        .clone()
        .expect("Page title must exist");

    let slug = title_to_slug(&title);

    let page_config = PageConfig {
        include: false,
        path_base: String::new(),
        path_name: String::new(),
        url_base: String::new(),
        url_name: slug,
        title,
        template: TemplateSelection::Default,
        picture_template: TemplateSelection::Default,
        picture_upload_profile: None,
        is_journal: page_block.is_journal,
        attrs: page_block.attrs.clone(),
        tags: page_block.tags.clone(),
        allow_embedding: AllowEmbed::Default,
        top_header_level: global_config.top_header_level,
        root_block: page.root_block,
    };

    let page = Arc::new(Mutex::new(page));

    let page_dy = Dynamic::from(page_config).into_shared();
    let mut scope = Scope::new();
    scope.push_dynamic("page", page_dy.clone());

    {
        let page = page.clone();
        engine.register_fn("autotag", move |input: rhai::Map, start_block: i64| {
            autotag(&page, input, start_block)
        });
    }

    {
        let page = page.clone();
        engine.register_fn(
            "each_block",
            move |context: NativeCallContext, max_depth: i64, callback: rhai::FnPtr| {
                each_block(context, &page, max_depth, callback)
            },
        );
    }

    engine
        .run_ast_with_scope(&mut scope, ast)
        .map_err(|e| eyre!("{e:?}"))?;

    drop(scope);
    drop(engine);

    let page_config = page_dy.cast::<PageConfig>();
    let page = Arc::try_unwrap(page).unwrap().into_inner().unwrap();
    Ok((page_config, page))
}
