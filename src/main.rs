mod make_pages;
mod parse_string;
mod roam_edn;
use anyhow::{anyhow, Context, Error, Result};
use std::fs::File;
use std::io::Read;
use structopt::StructOpt;

use make_pages::make_pages;
use roam_edn::Block;

#[derive(Debug, StructOpt)]
struct Config {
    #[structopt(short, long, env, default_value = "graph.edn")]
    file: String,

    #[structopt(short, long, env, default_value = "pages", help = "Output directory")]
    output: String,

    #[structopt(
        short,
        long,
        env,
        default_value = "export",
        help = "Page reference to indicate export inclusion (without hashtag or brackets)"
    )]
    filter: String,

    #[structopt(long="no-backlinks", env, parse(from_flag = std::ops::Not::not), help="Omit backlinks at the bottom of each page")]
    backlinks: bool,
}

fn title_to_slug(s: &str) -> String {
    String::from(s)
}

fn main() -> Result<()> {
    let config = Config::from_args();
    let mut f = File::open(&config.file).with_context(|| format!("Opening {}", config.file))?;
    let mut raw_data = String::new();
    f.read_to_string(&mut raw_data)?;

    let graph = roam_edn::Graph::from_edn(&raw_data)?;

    let pages = make_pages(&graph, &config.filter)?;

    println!(
        "Found {page_count} pages and {node_count} nodes",
        page_count = pages.len(),
        node_count = graph.blocks.len()
    );

    Ok(())
}
