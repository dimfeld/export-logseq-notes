use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Config {
    #[structopt(short, long, help = "Load the configuration from this path.")]
    pub config: Option<PathBuf>,

    #[structopt(short, long, env, default_value = "graph.edn")]
    pub file: PathBuf,

    #[structopt(short, long, env, default_value = "pages", help = "Output directory")]
    pub output: PathBuf,

    #[structopt(
        short,
        long,
        env,
        help = "Generate namespaced files in the same directory"
    )]
    pub flat_output: bool, // TODO

    #[structopt(
        short,
        long,
        env,
        default_value = "note-export",
        help = "Include pages with this hashtag or attribute. This reference will be omitted so that you can use a special tag that should not be rendered in the output. If a page references this as an attribute, the page's filename will be the value of the attribute."
    )]
    pub include: String,

    #[structopt(
        long,
        env,
        help = "Additional hashtags, links, and attributes to indicate a page should be included. Unlike the primary tag filter, this will not be removed from the output"
    )]
    pub also: Vec<String>,

    #[structopt(
        long,
        env,
        help = "Instead of using tags, include all pages, except for daily notes pages (controlled by --include-daily-logs) and pages with excluded tags"
    )]
    pub include_all: bool,

    #[structopt(long, env, help = "Make daily notes pages eligible to be included.")]
    pub allow_daily_notes: bool,

    #[structopt(
        long,
        env,
        help = "Hide pages that reference these hashtags, links, and attributes."
    )]
    pub exclude: Vec<String>,

    #[structopt(
        long,
        env,
        help = "When highlighting code, prefix class names with this value"
    )]
    pub highlight_class_prefix: Option<String>,

    #[structopt(
        long,
        env,
        help = "Template file",
        default_value = "templates/front_matter.tmpl"
    )]
    pub template: PathBuf,

    #[structopt(
        long = "ext",
        env,
        help = "Output file extension",
        default_value = "html"
    )]
    pub extension: String,

    #[structopt(
        short,
        long,
        env,
        help = "Attribute that indicates tags for a page",
        default_value = "Tags"
    )]
    pub tags_attr: String,

    #[structopt(long, env, help = "Tag a page with all included hashtags")]
    pub use_all_hashtags: bool; // TODO

    #[structopt(
        long,
        env,
        help = "If a block contains only links and hashtags, omit any links to unexported pages."
    )]
    pub filter_link_only_blocks: bool,

    #[structopt(long, env, help = "Include page embeds of non-exported pages")]
    pub include_all_page_embeds: bool, // TODO
}

impl Config {
    pub fn load() -> Config {
        let config_file = std::env::var("EXPORT_ROAM_NOTES_CONFIG")
            .unwrap_or_else(|_| "export-roam-notes.cfg".to_string());
        dotenv::from_filename(config_file).ok();

        let mut cfg = Config::from_args();

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

        cfg
    }
}
