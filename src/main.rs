mod config;
mod graph;
mod html;
mod links;
mod logseq;
mod make_pages;
mod page;
mod parse_string;
#[cfg(test)]
mod parse_string_tests;
mod roam_edn;
mod script;
mod string_builder;
mod syntax_highlight;
mod template;
use config::Config;
use eyre::{Result, WrapErr};
use std::fs::File;
use std::io::Read;
use zip::read::ZipArchive;

use crate::config::PkmProduct;
use crate::make_pages::make_pages_from_script;

fn main() -> Result<()> {
    color_eyre::install()?;

    let config = Config::load()?;

    let hbars = template::create(config.template.as_deref())?;
    let mut templates = template::DedupingTemplateRegistry::new(hbars);
    if let Some(path) = config.template.as_deref() {
        templates.add_file_with_key("default".to_string(), path)?;
    }

    let highlight_class_prefix = config.highlight_class_prefix.clone().map(|p| {
        // syntect requires a &`static str, so intentionally leak the string into the
        // static scope. Since we only ever create one of these, not a big deal.
        &*Box::leak::<'static>(p.into_boxed_str())
    });

    let highlighter = syntax_highlight::Highlighter::new(highlight_class_prefix);

    let graph = match config.product {
        PkmProduct::Roam => {
            let mut f = File::open(&config.path)
                .with_context(|| format!("Opening {}", config.path.display()))?;
            let mut raw_data = String::new();
            if config.path.extension().map(|e| e == "zip").unwrap_or(false) {
                let mut zip_reader = ZipArchive::new(f)?;
                let mut file = zip_reader.by_index(0)?;
                file.read_to_string(&mut raw_data)?;
            } else {
                f.read_to_string(&mut raw_data)?;
                drop(f);
            }
            roam_edn::graph_from_roam_edn(&raw_data)?
        }
        PkmProduct::Logseq => logseq::LogseqGraph::build(config.path.clone())?,
    };

    let page_count = make_pages_from_script(graph, templates, &highlighter, &config)?;

    println!("Wrote {page_count} pages");

    Ok(())
}
