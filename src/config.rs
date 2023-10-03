use std::{path::PathBuf, str::FromStr};

use eyre::{eyre, Result, WrapErr};
use serde::Deserialize;
use structopt::StructOpt;

#[derive(Debug, Default, StructOpt)]
struct CmdlineConfig {
    #[structopt(
        short,
        long,
        help = "Load the configuration from this path. Defaults to export-logseq-notes.toml"
    )]
    pub config: Option<PathBuf>,

    #[structopt(
        short,
        long,
        env,
        help = "The graph file to open. A Roam EDN file or a logseq directory. This can also be set in the config file."
    )]
    pub data: Option<PathBuf>,

    #[structopt(
        short,
        long,
        env,
        help = "Output directory. This can also be set in the config file."
    )]
    pub output: Option<PathBuf>,

    #[structopt(
        short,
        long,
        env,
        help = "The PKM product that produced the file. Defaults to Logseq"
    )]
    pub product: Option<PkmProduct>,

    #[structopt(
        long,
        help = "Write files so that there is no time when the contents are partially written."
    )]
    pub safe_write: bool,
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    /// Configure tracking of logseq file timestamps in a separate database. Defaults to true.
    pub track_logseq_timestamps: Option<bool>,

    /// The graph file to open. A Roam EDN file or a logseq directory. Must be specified if not
    /// given on the command line.
    pub data: Option<PathBuf>,

    /// Output directory. Must be specified if not given on the command line.
    pub output: Option<PathBuf>,

    /// Write files so that there is no time when the contents are partially written
    pub safe_write: Option<bool>,

    /// The script to run
    pub script: PathBuf,

    /// Data format to read. Defaults to Logseq
    pub product: Option<PkmProduct>,

    /// Base URL to apply to relative hyperlinks
    pub base_url: Option<String>,

    /// Skip rendering blocks with these attributes
    pub omit_attributes: Option<Vec<String>>,

    /// When highlighting code, prefix class names with this value
    pub highlight_class_prefix: Option<String>,

    /// Template file for each rendered page, if not set from the script
    pub template: Option<PathBuf>,

    /// Output file extension. Default: html
    pub extension: Option<String>,

    /// Attribute that indicates tags for a page
    pub tags_attr: Option<String>,

    /// If a block contains only links and hashtags, omit any references to unexported pages.
    pub filter_link_only_blocks: Option<bool>,

    // Syntax highlighter configuration
    pub class_bold: Option<String>,
    pub class_italic: Option<String>,
    pub class_strikethrough: Option<String>,
    pub class_highlight: Option<String>,
    pub class_blockquote: Option<String>,
    pub class_hr: Option<String>,
    pub class_block_embed: Option<String>,
    pub class_page_embed_container: Option<String>,
    pub class_page_embed_title: Option<String>,
    pub class_page_embed_content: Option<String>,
    pub class_attr_name: Option<String>,
    pub class_attr_value: Option<String>,
    pub class_heading1: Option<String>,
    pub class_heading2: Option<String>,
    pub class_heading3: Option<String>,
    pub class_heading4: Option<String>,

    /// Find the highest-level header in a page's content and treat it as header level 1.
    /// For example, if a page has `##` but not `#` in its markdown, then `##` will be
    /// header level 1, `###` will be header level 2, and so on.
    ///
    /// This can be used in conjunction with [top_header_level] force the highest level headers
    /// to be a specific level in the HTML output.
    ///
    /// A common setting would be `promote_headers = true` and `top_header_level = 2` to ensure
    /// that the top-level sections in the content will be `h2` and so on, regardless of whether
    /// the Markdown uses `#` or `##` for its top-level section headers.
    ///
    /// If omitted, this defaults to `false`.
    pub promote_headers: Option<bool>,

    /// Use this header level for the highest-level heading in the HTML output.
    /// For example, if this is 2, then header level 1 will generate an `h2`.
    ///
    /// This can be used in conjunction with [promote_headers] to force the highest level headers
    /// to be specific level in the HTML output.
    ///
    /// A common setting would be `promote_headers = true` and `top_header_level = 2` to ensure
    /// that the top-level sections in the content will be `h2` and so on.
    pub top_header_level: Option<usize>,

    /// Convert -- to &emdash; when generating HTML.
    pub convert_emdash: Option<bool>,

    /// Configuration for a Pic Store instance, to upload local images to the web.
    pub pic_store: Option<PicStoreConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PkmProduct {
    Roam,
    Logseq,
}

impl Default for PkmProduct {
    fn default() -> Self {
        Self::Logseq
    }
}

impl FromStr for PkmProduct {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roam" => Ok(Self::Roam),
            "logseq" => Ok(Self::Logseq),
            _ => Err(eyre!("Supported products are roam, logseq")),
        }
    }
}

pub struct Config {
    pub path: PathBuf,
    /// Track Logseq timestamps in a separate database. Defaults to true.
    pub track_logseq_timestamps: bool,
    pub output: PathBuf,
    pub script: PathBuf,
    pub safe_write: bool,
    pub product: PkmProduct,
    pub base_url: Option<String>,
    pub omit_attributes: Vec<String>,
    pub highlight_class_prefix: Option<String>,
    pub template: Option<PathBuf>,
    pub extension: String,
    pub tags_attr: Option<String>,
    pub filter_link_only_blocks: bool,
    pub class_bold: String,
    pub class_italic: String,
    pub class_strikethrough: String,
    pub class_highlight: String,
    pub class_blockquote: String,
    pub class_hr: String,
    pub class_block_embed: String,
    pub class_page_embed_container: String,
    pub class_page_embed_title: String,
    pub class_page_embed_content: String,
    pub class_attr_name: String,
    pub class_attr_value: String,
    pub class_heading1: String,
    pub class_heading2: String,
    pub class_heading3: String,
    pub class_heading4: String,
    pub convert_emdash: bool,

    pub promote_headers: bool,
    pub top_header_level: usize,

    pub pic_store: Option<PicStoreConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PicStoreConfig {
    /// The URL of the Pic Store instance to use.
    pub url: String,
    /// The API key to use for requests to Pic Store. This can also come from the PIC_STORE_KEY
    /// environment variable.
    pub api_key: Option<String>,
    /// The location prefix to use for Pic Store images. This will stack on top of any prefixes
    /// configured in the project or upload profile.
    pub location_prefix: Option<String>,
    /// The upload profile to use, if not the default one.
    pub upload_profile: Option<String>,
    /// A path to a template to generate <picture> tags. This can also be overridden from the page script.
    /// If not provided, a default template is used that generates a simple <picture> tag.
    pub template: Option<PathBuf>,
}

fn merge_required<T>(name: &str, first: Option<T>, second: Option<T>) -> Result<T> {
    first
        .or(second)
        .ok_or_else(|| eyre!("The {} option is required", name))
}

fn merge_default<T: Default>(first: Option<T>, second: Option<T>) -> T {
    first.or(second).unwrap_or_default()
}

impl Config {
    pub fn load() -> Result<Config> {
        dotenv::dotenv().ok();

        let cmdline_cfg = CmdlineConfig::from_args();
        let config_file = std::fs::read_to_string(
            cmdline_cfg
                .config
                .as_ref()
                .unwrap_or(&PathBuf::from("export-logseq-notes.toml")),
        )
        .context("Failed to open config file")?;

        let mut file_cfg: FileConfig = toml::from_str(&config_file)?;

        if let Some(pc) = file_cfg.pic_store.as_mut() {
            if pc.api_key.is_none() {
                let key = match std::env::var("PIC_STORE_KEY") {
                    Ok(k) => k,
                    Err(_) => {
                        return Err(eyre!(
                            "The PIC_STORE_KEY environment variable or the pic_store.api_key config key must be set to use Pic Store"
                        ))
                    }
                };

                pc.api_key = Some(key);
            }
        }

        let mut cfg = Config {
            path: merge_required("data", cmdline_cfg.data, file_cfg.data)?,
            track_logseq_timestamps: file_cfg.track_logseq_timestamps.unwrap_or(true),
            output: merge_required("output", cmdline_cfg.output, file_cfg.output)?,
            script: file_cfg.script,
            product: merge_default(cmdline_cfg.product, file_cfg.product),
            safe_write: cmdline_cfg.safe_write || file_cfg.safe_write.unwrap_or(false),
            base_url: file_cfg.base_url,
            omit_attributes: file_cfg.omit_attributes.unwrap_or_default(),
            highlight_class_prefix: file_cfg.highlight_class_prefix,
            template: file_cfg.template,
            extension: file_cfg.extension.unwrap_or_default(),
            tags_attr: file_cfg.tags_attr,
            filter_link_only_blocks: file_cfg.filter_link_only_blocks.unwrap_or_default(),
            class_bold: file_cfg.class_bold.unwrap_or_default(),
            class_italic: file_cfg.class_italic.unwrap_or_default(),
            class_strikethrough: file_cfg.class_strikethrough.unwrap_or_default(),
            class_highlight: file_cfg.class_highlight.unwrap_or_default(),
            class_blockquote: file_cfg.class_blockquote.unwrap_or_default(),
            class_hr: file_cfg.class_hr.unwrap_or_default(),
            class_block_embed: file_cfg.class_block_embed.unwrap_or_default(),
            class_page_embed_container: file_cfg.class_page_embed_container.unwrap_or_default(),
            class_page_embed_title: file_cfg.class_page_embed_title.unwrap_or_default(),
            class_page_embed_content: file_cfg.class_page_embed_content.unwrap_or_default(),
            class_attr_name: file_cfg.class_attr_name.unwrap_or_default(),
            class_attr_value: file_cfg.class_attr_value.unwrap_or_default(),
            class_heading1: file_cfg.class_heading1.unwrap_or_default(),
            class_heading2: file_cfg.class_heading2.unwrap_or_default(),
            class_heading3: file_cfg.class_heading3.unwrap_or_default(),
            class_heading4: file_cfg.class_heading4.unwrap_or_default(),
            convert_emdash: file_cfg.convert_emdash.unwrap_or_default(),
            promote_headers: file_cfg.promote_headers.unwrap_or_default(),
            top_header_level: file_cfg.top_header_level.unwrap_or(1),
            pic_store: file_cfg.pic_store,
        };

        // Make sure base url starts and ends with a slash
        cfg.base_url = cfg.base_url.map(|url| {
            let prefix = if url.starts_with('/') { "" } else { "/" };
            let suffix = if url.ends_with('/') { "" } else { "/" };

            format!("{prefix}{url}{suffix}")
        });

        Ok(cfg)
    }
}
