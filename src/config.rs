use eyre::{eyre, Result, WrapErr};
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use structopt::StructOpt;

#[derive(Debug, Default, Deserialize, StructOpt)]
struct InputConfig {
    #[structopt(
        short,
        long,
        help = "Load the configuration from this path. Defaults to export-logseq-notes.toml"
    )]
    pub config: Option<PathBuf>,

    #[structopt(
        long,
        help = "Disable tracking of Logseq timestamps in a separate database"
    )]
    pub no_track_logseq_timestamps: Option<bool>,

    #[structopt(
        short,
        long,
        env,
        help = "The graph file to open. A Roam EDN file or a logseq directory"
    )]
    pub data: Option<PathBuf>,

    #[structopt(short, long, env, help = "Output directory")]
    pub output: Option<PathBuf>,

    #[structopt(short, long, env, help = "The script to run")]
    pub script: Option<PathBuf>,

    #[structopt(long, env, help = "Data format to read")]
    pub product: Option<PkmProduct>,

    #[structopt(long, env, help = "Base URL to apply to relative hyperlinks")]
    pub base_url: Option<String>,

    #[structopt(long, env, help = "Skip rendering blocks with these attributes")]
    pub omit_attributes: Option<Vec<String>>,

    #[structopt(
        long,
        env,
        help = "When highlighting code, prefix class names with this value"
    )]
    pub highlight_class_prefix: Option<String>,

    #[structopt(
        long,
        env,
        help = "Template file for each rendered page, if not set from the script"
    )]
    pub template: Option<PathBuf>,

    #[structopt(long = "ext", env, help = "Output file extension. Default: html")]
    pub extension: Option<String>,

    #[structopt(short, long, env, help = "Attribute that indicates tags for a page")]
    pub tags_attr: Option<String>,

    #[structopt(
        long,
        env,
        help = "If a block contains only links and hashtags, omit any references to unexported pages."
    )]
    pub filter_link_only_blocks: Option<bool>,

    // Syntax highlighter configuration
    #[structopt(long, env)]
    pub class_bold: Option<String>,
    #[structopt(long, env)]
    pub class_italic: Option<String>,
    #[structopt(long, env)]
    pub class_strikethrough: Option<String>,
    #[structopt(long, env)]
    pub class_highlight: Option<String>,
    #[structopt(long, env)]
    pub class_blockquote: Option<String>,
    #[structopt(long, env)]
    pub class_hr: Option<String>,
    #[structopt(long, env)]
    pub class_block_embed: Option<String>,
    #[structopt(long, env)]
    pub class_page_embed_container: Option<String>,
    #[structopt(long, env)]
    pub class_page_embed_title: Option<String>,
    #[structopt(long, env)]
    pub class_page_embed_content: Option<String>,
    #[structopt(long, env)]
    pub class_attr_name: Option<String>,
    #[structopt(long, env)]
    pub class_attr_value: Option<String>,
    #[structopt(long, env)]
    pub class_heading1: Option<String>,
    #[structopt(long, env)]
    pub class_heading2: Option<String>,
    #[structopt(long, env)]
    pub class_heading3: Option<String>,
    #[structopt(long, env)]
    pub class_heading4: Option<String>,
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
}

fn merge_required<T>(name: &str, first: Option<T>, second: Option<T>) -> Result<T> {
    first
        .or(second)
        .ok_or_else(|| eyre!("The {} option is required", name))
}

fn merge_default<T: Default>(first: Option<T>, second: Option<T>) -> T {
    first.or(second).unwrap_or_default()
}

fn merge_or<T>(first: Option<T>, second: Option<T>, fallback: T) -> T {
    first.or(second).unwrap_or(fallback)
}

impl Config {
    pub fn load() -> Result<Config> {
        dotenv::dotenv().ok();

        // Read both from the arguments and from the config file.

        let cmdline_cfg = InputConfig::from_args();
        let config_file_path = cmdline_cfg.config.as_ref();
        let config_file = std::fs::File::open(
            config_file_path.unwrap_or(&PathBuf::from("export-logseq-notes.toml")),
        );

        let file_cfg = match (config_file, &cmdline_cfg.config) {
            (Ok(mut f), _) => {
                let mut data = String::new();
                f.read_to_string(&mut data)?;
                let cfg: InputConfig = toml::from_str(&data)?;
                cfg
            }
            (Err(e), Some(_)) => {
                // A config was explicitly specified, so it's an error to not find it.
                return Err(e).context("Failed to open config file");
            }
            (Err(_), None) => {
                // The user didn't spcify a config filename, so it's ok if the file doesn't
                // exist.
                InputConfig::default()
            }
        };

        let mut cfg = Config {
            path: merge_required("data", cmdline_cfg.data, file_cfg.data)?,
            track_logseq_timestamps: !merge_or(
                cmdline_cfg.no_track_logseq_timestamps,
                file_cfg.no_track_logseq_timestamps,
                false,
            ),
            output: merge_required("output", cmdline_cfg.output, file_cfg.output)?,
            script: merge_required("script", cmdline_cfg.script, file_cfg.script)?,
            product: merge_default(cmdline_cfg.product, file_cfg.product),
            base_url: cmdline_cfg.base_url.or(file_cfg.base_url),
            omit_attributes: merge_default(cmdline_cfg.omit_attributes, file_cfg.omit_attributes),
            highlight_class_prefix: cmdline_cfg
                .highlight_class_prefix
                .or(file_cfg.highlight_class_prefix),
            template: cmdline_cfg.template.or(file_cfg.template),
            extension: merge_default(cmdline_cfg.extension, file_cfg.extension),
            tags_attr: cmdline_cfg.tags_attr.or(file_cfg.tags_attr),
            filter_link_only_blocks: merge_default(
                cmdline_cfg.filter_link_only_blocks,
                file_cfg.filter_link_only_blocks,
            ),
            class_bold: merge_default(cmdline_cfg.class_bold, file_cfg.class_bold),
            class_italic: merge_default(cmdline_cfg.class_italic, file_cfg.class_italic),
            class_strikethrough: merge_default(
                cmdline_cfg.class_strikethrough,
                file_cfg.class_strikethrough,
            ),
            class_highlight: merge_default(cmdline_cfg.class_highlight, file_cfg.class_highlight),
            class_blockquote: merge_default(
                cmdline_cfg.class_blockquote,
                file_cfg.class_blockquote,
            ),
            class_hr: merge_default(cmdline_cfg.class_hr, file_cfg.class_hr),
            class_block_embed: merge_default(
                cmdline_cfg.class_block_embed,
                file_cfg.class_block_embed,
            ),
            class_page_embed_container: merge_default(
                cmdline_cfg.class_page_embed_container,
                file_cfg.class_page_embed_container,
            ),
            class_page_embed_title: merge_default(
                cmdline_cfg.class_page_embed_title,
                file_cfg.class_page_embed_title,
            ),
            class_page_embed_content: merge_default(
                cmdline_cfg.class_page_embed_content,
                file_cfg.class_page_embed_content,
            ),
            class_attr_name: merge_default(cmdline_cfg.class_attr_name, file_cfg.class_attr_name),
            class_attr_value: merge_default(
                cmdline_cfg.class_attr_value,
                file_cfg.class_attr_value,
            ),
            class_heading1: merge_default(cmdline_cfg.class_heading1, file_cfg.class_heading1),
            class_heading2: merge_default(cmdline_cfg.class_heading2, file_cfg.class_heading2),
            class_heading3: merge_default(cmdline_cfg.class_heading3, file_cfg.class_heading3),
            class_heading4: merge_default(cmdline_cfg.class_heading4, file_cfg.class_heading4),
        };

        // Make sure base url starts and ends with a slash
        cfg.base_url = cfg.base_url.map(|url| {
            let prefix = if url.starts_with('/') { "" } else { "/" };
            let suffix = if url.ends_with('/') { "" } else { "/" };

            format!("{}{}{}", prefix, url, suffix)
        });

        Ok(cfg)
    }
}
