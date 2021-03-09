use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;
use structopt::StructOpt;
use toml;

#[derive(Debug, Default, Deserialize, StructOpt)]
struct InputConfig {
    #[structopt(
        short,
        long,
        help = "Load the configuration from this path. Defaults to export-roam-notes.toml"
    )]
    pub config: Option<PathBuf>,

    #[structopt(
        short,
        long,
        env,
        help = "The graph file to open. Either an EDN file or a ZIP containing one"
    )]
    pub file: Option<PathBuf>,

    #[structopt(short, long, env, help = "Output directory")]
    pub output: Option<PathBuf>,

    #[structopt(long, env, help = "Base URL to apply to relative hyperlinks")]
    pub base_url: Option<String>,

    #[structopt(
        short,
        long,
        env,
        help = "Generate namespaces in their own directories (Not implemented yet)"
    )]
    pub namespace_dirs: Option<bool>, // TODO

    #[structopt(
        short,
        long,
        env,
        help = "Include pages with this hashtag or attribute. This reference will be omitted so that you can use a special tag that should not be rendered in the output. If a page references this as an attribute, the page's filename will be the value of the attribute."
    )]
    pub include: Option<String>,

    #[structopt(
        long,
        env,
        help = "Additional hashtags, links, and attributes to indicate a page should be included. Unlike the primary tag filter, these will not be removed from the output"
    )]
    pub also: Option<Vec<String>>,

    #[structopt(
        long,
        env,
        help = "Instead of using tags, include all pages, except for daily notes pages (controlled by --allow-daily-notes) and pages with excluded tags"
    )]
    pub include_all: Option<bool>,

    #[structopt(
        long,
        env,
        help = "Make daily notes pages eligible to be included. The other inclusion criteria still apply."
    )]
    pub allow_daily_notes: Option<bool>,

    #[structopt(
        long,
        env,
        help = "Hide pages that reference these hashtags, links, and attributes."
    )]
    pub exclude: Option<Vec<String>>,

    #[structopt(
        long,
        env,
        help = "Exclude these values from the page template's `tags` list"
    )]
    pub exclude_tags: Option<Vec<String>>,

    #[structopt(long, env, help = "Skip rendering blocks with these attributes")]
    pub omit_attributes: Option<Vec<String>>,

    #[structopt(
        long,
        env,
        help = "When highlighting code, prefix class names with this value"
    )]
    pub highlight_class_prefix: Option<String>,

    #[structopt(long, env, help = "Template file for each rendered page")]
    pub template: Option<PathBuf>,

    #[structopt(long = "ext", env, help = "Output file extension. Default: html")]
    pub extension: Option<String>,

    #[structopt(short, long, env, help = "Attribute that indicates tags for a page")]
    pub tags_attr: Option<String>,

    #[structopt(long, env, help = "Tag a page with all included hashtags")]
    pub use_all_hashtags: Option<bool>,

    #[structopt(
        long,
        env,
        help = "If a block contains only links and hashtags, omit any references to unexported pages."
    )]
    pub filter_link_only_blocks: Option<bool>,

    #[structopt(long, env, help = "Include page embeds of non-exported pages")]
    pub include_all_page_embeds: Option<bool>,
}

pub struct Config {
    pub file: PathBuf,
    pub output: PathBuf,
    pub base_url: Option<String>,
    pub namespace_dirs: bool, // TODO
    pub include: String,
    pub also: Vec<String>,
    pub include_all: bool,
    pub allow_daily_notes: bool,
    pub exclude: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub omit_attributes: Vec<String>,
    pub highlight_class_prefix: Option<String>,
    pub template: PathBuf,
    pub extension: String,
    pub tags_attr: Option<String>,
    pub use_all_hashtags: bool,
    pub filter_link_only_blocks: bool,
    pub include_all_page_embeds: bool, // TODO
}

fn merge_required<T>(name: &str, first: Option<T>, second: Option<T>) -> Result<T> {
    first
        .or(second)
        .ok_or_else(|| anyhow!("The {} option is required", name))
}

fn merge_default<T: Default>(first: Option<T>, second: Option<T>) -> T {
    first.or(second).unwrap_or_else(T::default)
}

impl Config {
    pub fn load() -> Result<Config> {
        dotenv::dotenv().ok();

        // Read both from the arguments and from the config file.

        let cmdline_cfg = InputConfig::from_args();
        let config_file_path = cmdline_cfg.config.as_ref();
        let config_file = std::fs::File::open(
            config_file_path.unwrap_or(&PathBuf::from("export-roam-notes.toml")),
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
            file: merge_required("file", cmdline_cfg.file, file_cfg.file)?,
            output: merge_required("output", cmdline_cfg.output, file_cfg.output)?,
            base_url: cmdline_cfg.base_url.or(file_cfg.base_url),
            namespace_dirs: merge_default(cmdline_cfg.namespace_dirs, file_cfg.namespace_dirs),
            include: merge_required("include", cmdline_cfg.include, file_cfg.include)?,
            also: merge_default(cmdline_cfg.also, file_cfg.also),
            include_all: merge_default(cmdline_cfg.include_all, file_cfg.include_all),
            allow_daily_notes: merge_default(
                cmdline_cfg.allow_daily_notes,
                file_cfg.allow_daily_notes,
            ),
            exclude: merge_default(cmdline_cfg.exclude, file_cfg.exclude),
            exclude_tags: merge_default(cmdline_cfg.exclude_tags, file_cfg.exclude_tags),
            omit_attributes: merge_default(cmdline_cfg.omit_attributes, file_cfg.omit_attributes),
            highlight_class_prefix: cmdline_cfg
                .highlight_class_prefix
                .or(file_cfg.highlight_class_prefix),
            template: merge_default(cmdline_cfg.template, file_cfg.template),
            extension: merge_default(cmdline_cfg.extension, file_cfg.extension),
            tags_attr: cmdline_cfg.tags_attr.or(file_cfg.tags_attr),
            use_all_hashtags: merge_default(
                cmdline_cfg.use_all_hashtags,
                file_cfg.use_all_hashtags,
            ),
            filter_link_only_blocks: merge_default(
                cmdline_cfg.filter_link_only_blocks,
                file_cfg.filter_link_only_blocks,
            ),
            include_all_page_embeds: merge_default(
                cmdline_cfg.include_all_page_embeds,
                file_cfg.include_all_page_embeds,
            ),
        };

        // For environment variables, handle comma separated strings for vectors
        cfg.also = cfg
            .also
            .iter()
            .flat_map(|w| w.split(',').map(|t| String::from(t.trim())))
            .collect::<Vec<_>>();

        cfg.exclude = cfg
            .exclude
            .iter()
            .flat_map(|w| w.split(',').map(|t| String::from(t.trim())))
            .collect::<Vec<_>>();

        cfg.exclude_tags = cfg
            .exclude_tags
            .iter()
            .flat_map(|w| w.split(',').map(|t| String::from(t.trim())))
            .collect::<Vec<_>>();

        // Make sure base url starts and ends with a slash
        cfg.base_url = cfg.base_url.map(|url| {
            let prefix = if url.starts_with('/') { "" } else { "/" };
            let suffix = if url.ends_with('/') { "" } else { "/" };

            format!("{}{}{}", prefix, url, suffix)
        });

        Ok(cfg)
    }
}
