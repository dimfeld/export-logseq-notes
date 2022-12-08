mod config;
mod content;
mod graph;
mod html;
mod image;
mod logseq;
mod make_pages;
mod page;
mod parse_string;
#[cfg(test)]
mod parse_string_tests;
mod pic_store;
mod roam_edn;
mod script;
mod string_builder;
mod syntax_highlight;
mod template;
use std::{fs::File, io::Read};

use config::Config;
use eyre::{Result, WrapErr};
use zip::read::ZipArchive;

use crate::{config::PkmProduct, make_pages::make_pages_from_script};

fn main() -> Result<()> {
    color_eyre::install()?;

    let config = Config::load()?;

    let hbars = template::create(config.template.as_deref())?;
    let mut templates = template::DedupingTemplateRegistry::new(hbars);
    if let Some(path) = config.template.as_deref() {
        templates.add_file_with_key("default".to_string(), path)?;
    }

    if let Some(path) = config
        .pic_store
        .as_ref()
        .and_then(|ps| ps.template.as_deref())
    {
        templates.add_file_with_key("default_picture".to_string(), path)?;
    } else {
        // Use the default picture template if none was provided.
        templates.add_template(
            "default_picture".to_string(),
            image::DEFAULT_PICTURE_TEMPLATE.to_string(),
        )?;
    }

    let highlight_class_prefix = config.highlight_class_prefix.clone().map(|p| {
        // syntect requires a &`static str, so intentionally leak the string into the
        // static scope. Since we only ever create one of these, not a big deal.
        &*Box::leak::<'static>(p.into_boxed_str())
    });

    let highlighter = syntax_highlight::Highlighter::new(highlight_class_prefix);

    let metadata_db = (config.track_logseq_timestamps || config.pic_store.is_some())
        .then(|| {
            let base_dir = match config.product {
                PkmProduct::Roam => dirs::config_dir().unwrap().join("export-logseq-notes"),
                PkmProduct::Logseq => config.path.clone(),
            };

            logseq::db::MetadataDb::new(base_dir)
        })
        .transpose()?;

    let (content_style, explicit_ordering, parsed_pages) = match config.product {
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
        PkmProduct::Logseq => logseq::LogseqGraph::build(
            config.path.clone(),
            if config.track_logseq_timestamps {
                metadata_db.clone()
            } else {
                None
            },
        )?,
    };

    let (wrote, skipped) = make_pages_from_script(
        parsed_pages,
        content_style,
        explicit_ordering,
        templates,
        &highlighter,
        &config,
        metadata_db,
    )?;

    println!("Wrote {wrote} pages, skipped {skipped} up-to-date");

    Ok(())
}
