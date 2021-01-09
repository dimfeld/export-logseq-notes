mod roam_edn;
use anyhow::{anyhow, Context, Error, Result};
use std::fs::File;
use std::io::Read;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Config {
    #[structopt(short, long, env, default_value = "graph.edn")]
    file: String,

    #[structopt(short, long, env, default_value = "pages")]
    output: String,

    #[structopt(long="no-backlinks", env, parse(from_flag = std::ops::Not::not), help="Omit backlinks at the bottom of each page")]
    backlinks: bool,
}

fn main() -> Result<()> {
    let config = Config::from_args();
    let mut f = File::open(&config.file).with_context(|| format!("Opening {}", config.file))?;
    let mut raw_data = String::new();
    f.read_to_string(&mut raw_data)?;

    let graph = roam_edn::Graph::from_edn(&raw_data)?;

    let mut pages = 0;
    graph
        .nodes
        .iter()
        .filter_map(|(_, node)| node.title.as_ref())
        .for_each(|title| {
            pages += 1;
            println!("{}", title);
        });

    println!(
        "Found {page_count} pages and {node_count} nodes",
        page_count = pages,
        node_count = graph.nodes.len()
    );

    Ok(())
}
