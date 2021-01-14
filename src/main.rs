mod html;
mod make_pages;
mod parse_string;
mod parse_string_tests;
mod roam_edn;
mod syntax_highlight;
mod template;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use structopt::StructOpt;

use make_pages::make_pages;

#[derive(Debug, StructOpt)]
struct Config {
    #[structopt(short, long, env, default_value = "graph.edn")]
    file: String,

    #[structopt(short, long, env, default_value = "pages", help = "Output directory")]
    output: PathBuf,

    #[structopt(
        short,
        long,
        env,
        default_value = "note-export",
        help = "Page reference to indicate export inclusion (without hashtag or brackets)"
    )]
    tag: String,

    #[structopt(long="no-backlinks", env, parse(from_flag = std::ops::Not::not), help="Omit backlinks at the bottom of each page")]
    backlinks: bool, // TODO

    #[structopt(
        long,
        env,
        help = "When highlighting code, prefix class names with this value"
    )]
    highlight_class_prefix: Option<String>,

    #[structopt(
        long,
        env,
        help = "Template file",
        default_value = "templates/front_matter.tmpl"
    )]
    template: PathBuf,

    #[structopt(
        long = "ext",
        env,
        help = "Output file extension",
        default_value = "html"
    )]
    extension: String,

    #[structopt(
        long,
        env,
        help = "If a block contains just a single link and it is to a non-exported page, omit the block"
    )]
    omit_blocks_with_only_unexported_links: bool, // TODO

    #[structopt(long, env, help = "Include page embeds of non-exported pages")]
    include_all_page_embeds: bool, // TODO
}

fn main() -> Result<()> {
    let config = Config::from_args();
    let mut f = File::open(&config.file).with_context(|| format!("Opening {}", config.file))?;
    let mut raw_data = String::new();
    f.read_to_string(&mut raw_data)?;

    let hbars = template::create(&config.template)?;

    let highlight_class_prefix = config.highlight_class_prefix.map(|p| {
        // syntect requires a &`static str, so intentionally leak the string into the
        // static scope. Since we only ever create one of these, not a big deal.
        &*Box::leak::<'static>(p.into_boxed_str())
    });

    let highlighter = syntax_highlight::Highlighter::new(highlight_class_prefix);

    let graph = roam_edn::Graph::from_edn(&raw_data)?;
    let pages = make_pages(
        &graph,
        &hbars,
        &highlighter,
        &config.tag,
        &config.output,
        &config.extension,
    )?;

    println!("Wrote {page_count} pages", page_count = pages.len());

    Ok(())
}
