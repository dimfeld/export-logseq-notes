mod html;
mod make_pages;
mod parse_string;
mod parse_string_tests;
mod roam_edn;
mod syntax_highlight;
use anyhow::{anyhow, Context, Error, Result};
use fxhash::FxHashSet;
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
    output: std::path::PathBuf,

    #[structopt(
        short,
        long,
        env,
        default_value = "export",
        help = "Page reference to indicate export inclusion (without hashtag or brackets)"
    )]
    tag: String,

    #[structopt(long="no-backlinks", env, parse(from_flag = std::ops::Not::not), help="Omit backlinks at the bottom of each page")]
    backlinks: bool,

    #[structopt(
        long,
        env,
        help = "When hihglighting code, prefix class names with this value"
    )]
    highlight_class_prefix: Option<String>,
}

fn main() -> Result<()> {
    let config = Config::from_args();
    let mut f = File::open(&config.file).with_context(|| format!("Opening {}", config.file))?;
    let mut raw_data = String::new();
    f.read_to_string(&mut raw_data)?;

    let highlight_class_prefix = config.highlight_class_prefix.map(|p| {
        // syntect requires a &`static str, so intentionally leak the string into the
        // static scope. Since we only ever create one of these, not a big deal.
        &*Box::leak::<'static>(p.into_boxed_str())
    });

    let highlighter = syntax_highlight::Highlighter::new(highlight_class_prefix);

    let graph = roam_edn::Graph::from_edn(&raw_data)?;
    let pages = make_pages(&graph, &highlighter, &config.tag, &config.output)?;

    let mut block_count = 0;
    // for (id, block) in &graph.blocks {
    //     if block.string.is_empty() || exported_page_ids.get(&block.page).is_none() {
    //         continue;
    //     }

    //     block_count += 1;

    //     let parse_result = match parse_string::parse(&block.string) {
    //         Ok(e) => format!("Parsed: {:?}", e),
    //         Err(e) => format!("Error: {:?}", e),
    //     };

    //     print!("Input: {}\n{}\n\n", block.string, parse_result);
    // }

    println!(
        "Found {page_count} pages and {block_count} nodes",
        page_count = pages.len(),
        block_count = block_count
    );

    Ok(())
}
