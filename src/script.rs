use fxhash::{FxHashMap, FxHashSet};
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
use rhai::{def_package, plugin::*, CustomType, TypeBuilder};
use smallvec::SmallVec;

#[derive(Debug, Clone, Default)]
pub enum PageInclude {
    /// Render only pages included via include_block
    #[default]
    Partial,
    /// Render the entire page
    All,
    /// Omit the entire page
    None,
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
    pub attrs: FxHashMap<String, String>,
    pub tags: FxHashSet<String>,

    pub include: PageInclude,
    pub include_blocks: SmallVec<[(usize, BlockInclude); 4]>,
    pub exclude_blocks: SmallVec<[usize; 4]>,

    pub allow_embedding: AllowEmbed,
}

#[export_module]
pub mod rhai_page {
    use rhai::FnPtr;

    pub type Page = PageConfig;

    #[rhai_fn(get = "path_base", pure)]
    pub fn get_path_base(page: &mut PageConfig) -> String {
        page.path_base.to_string()
    }

    #[rhai_fn(set = "path_base")]
    pub fn set_path_base(page: &mut PageConfig, value: String) {
        page.path_base = value;
    }

    #[rhai_fn(get = "path_name", pure)]
    pub fn get_path_name(page: &mut PageConfig) -> String {
        page.path_name.to_string()
    }

    #[rhai_fn(set = "path_name")]
    pub fn set_path_name(page: &mut PageConfig, value: String) {
        page.path_name = value;
    }

    #[rhai_fn(get = "url_base", pure)]
    pub fn get_url_base(page: &mut PageConfig) -> String {
        page.url_base.to_string()
    }

    #[rhai_fn(set = "url_base")]
    pub fn set_url_base(page: &mut PageConfig, value: String) {
        page.url_base = value;
    }

    #[rhai_fn(get = "url_name", pure)]
    pub fn get_url_name(page: &mut PageConfig) -> String {
        page.url_name.to_string()
    }

    #[rhai_fn(set = "url_name")]
    pub fn set_url_name(page: &mut PageConfig, value: String) {
        page.url_name = value;
    }

    #[rhai_fn(get = "allow_embedding")]
    pub fn get_allow_embedding(page: &mut PageConfig) -> AllowEmbed {
        page.allow_embedding
    }

    #[rhai_fn(set = "allow_embedding")]
    pub fn set_allow_embedding(page: &mut PageConfig, value: AllowEmbed) {
        page.allow_embedding = value
    }

    #[rhai_fn(global)]
    pub fn allow_render(page: &mut PageConfig, value: PageInclude) {
        page.include = value;
    }

    #[rhai_fn(global)]
    pub fn each_block(
        context: NativeCallContext,
        page: &mut PageConfig,
        max_depth: i64,
        callback: FnPtr,
    ) {
        // Walk the blocks up to max_depth and call the callback for each one.
        todo!()
    }

    #[rhai_fn(global)]
    pub fn include_block(page: &mut PageConfig, block_id: i64, include: BlockInclude) {
        page.include_blocks.push((block_id as usize, include));
    }

    #[rhai_fn(global)]
    pub fn exclude_block(page: &mut PageConfig, block_id: i64) {
        page.exclude_blocks.push(block_id as usize)
    }
}
