use crate::config::Config;
use crate::graph::{Graph, ParsedPage};
use crate::page::{IdSlugUid, ManifestItem, Page, TitleSlugUid};
use crate::parse_string::ContentStyle;
use crate::script::{run_script_on_page, AllowEmbed, TemplateSelection};
use crate::syntax_highlight;
use ahash::{HashMap, HashSet};
use eyre::{eyre, Result, WrapErr};
use itertools::Itertools;
use rayon::prelude::*;
use rhai::packages::Package;
use rhai::Engine;
use serde::Serialize;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Serialize, Debug)]
struct TemplateArgs<'a> {
    title: &'a str,
    body: &'a str,
    tags: Vec<&'a str>,
    created_time: usize,
    edited_time: usize,
}

pub fn title_to_slug(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c == '/' || c == '-' || c == ':')
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphabetic() || c.is_ascii_digit())
                .flat_map(|c| c.to_lowercase())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .join("_")
}

fn create_path(page_base: &str, default_base: &str, filename: &str) -> String {
    let base = if page_base.is_empty() {
        default_base
    } else {
        page_base
    };

    format!("{base}/{filename}")
}

pub fn make_pages_from_script(
    pages: Vec<ParsedPage>,
    content_style: ContentStyle,
    explicit_ordering: bool,
    mut templates: crate::template::DedupingTemplateRegistry,
    highlighter: &syntax_highlight::Highlighter,
    config: &Config,
) -> Result<(usize, usize)> {
    let package = crate::script::ParsePackage::new();
    let mut parse_engine = Engine::new_raw();
    package.register_into_engine(&mut parse_engine);

    let ast = parse_engine
        .compile_file(config.script.clone())
        .map_err(|e| eyre!("{e:?}"))
        .wrap_err("Parsing script")?;

    let mut pages = pages
        .into_iter()
        .map(|parsed_page| {
            let (page_config, page_blocks) =
                run_script_on_page(&package, &ast, parsed_page).wrap_err("Running script")?;
            let slug = create_path(
                page_config.url_base.as_str(),
                config.base_url.as_deref().unwrap_or(""),
                page_config.url_name.as_str(),
            );

            Ok::<_, eyre::Report>((page_config, page_blocks, slug))
        })
        .filter(|result| match result {
            Ok((page, _, _)) => page.include || page.allow_embedding == AllowEmbed::Yes,
            _ => true,
        })
        .collect::<Result<Vec<_>>>()?;

    let page_templates = pages
        .iter_mut()
        .map(|(page, _, _)| {
            let template_key = match std::mem::take(&mut page.template) {
                TemplateSelection::Default => {
                    if config.template.is_none() {
                        return Err(eyre!("Config has no default template, but page {} does not specify a template", page.title));
                    }
                    "default".to_string()
                },
                TemplateSelection::File(f) => templates.add_file(&PathBuf::from(f))?,
                TemplateSelection::Value(v) => templates.add_string(v)?,
            };

            Ok::<_, eyre::Report>((page.root_block, template_key))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let pages_by_title = pages
        .iter()
        .map(|(p, blocks, slug)| {
            (
                p.title.clone(),
                IdSlugUid {
                    id: p.root_block,
                    include: p.include,
                    allow_embed: match (p.include, p.allow_embedding) {
                        (true, AllowEmbed::Yes | AllowEmbed::Default) => true,
                        (true, AllowEmbed::No) => false,
                        (false, AllowEmbed::Yes) => true,
                        (false, AllowEmbed::No | AllowEmbed::Default) => false,
                    },
                    slug: slug.clone(),
                    uid: blocks.blocks.get(&p.root_block).unwrap().uid.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let pages_by_id = pages_by_title
        .iter()
        .map(|(title, isu)| {
            (
                isu.id,
                TitleSlugUid {
                    title: title.clone(),
                    slug: isu.slug.clone(),
                    uid: isu.uid.clone(),
                    include: isu.include,
                    allow_embed: isu.allow_embed,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let default_output_dir = config.output.to_string_lossy();

    let omitted_attributes = config
        .omit_attributes
        .iter()
        .map(|x| x.as_str())
        .collect::<HashSet<_>>();

    let handlebars = templates.into_inner();

    let mut graph = Graph::new(content_style, explicit_ordering);
    for (_, blocks, _) in pages.iter_mut() {
        let blocks = std::mem::take(&mut blocks.blocks);
        for (_, block) in blocks {
            graph.add_block(block);
        }
    }

    let results = pages
        .into_par_iter()
        .map(|(page_config, blocks, slug)| {
            if !page_config.include {
                return Ok(None);
            }

            let filename = if page_config.path_name.is_empty() {
                format!("{}.{}", page_config.url_name, config.extension)
            } else {
                page_config.path_name
            };

            let output_path = create_path(
                page_config.path_base.as_str(),
                default_output_dir.as_ref(),
                &filename,
            );

            let page = Page {
                id: page_config.root_block,
                title: page_config.title,
                slug: slug.as_str(),
                latest_found_edit_time: std::cell::Cell::new(0),
                graph: &graph,
                config,
                pages_by_title: &pages_by_title,
                pages_by_id: &pages_by_id,
                omitted_attributes: &omitted_attributes,
                highlighter,
            };

            let block = graph.blocks.get(&page.id).unwrap();

            let (rendered, hashtags) = page.render()?;

            if rendered.is_empty() {
                return Ok(None);
            }

            let mut tags = page_config
                .tags
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>();
            tags.sort_by_key(|k| k.to_lowercase());
            tags.dedup();

            let lower_tags = tags
                .iter()
                .map(|t| t.to_lowercase())
                .collect::<HashSet<_>>();

            // println!("{:?} {:?}", title, tags);

            let mut popped = false;
            let mut title_components = page.title.split('/').collect::<SmallVec<[&str; 3]>>();
            while title_components.len() > 1
                && lower_tags.contains(&title_components[0].to_lowercase())
            {
                popped = true;
                title_components.remove(0);
            }

            let final_title = if popped {
                Cow::from(title_components.iter().join("/"))
            } else {
                Cow::from(&page.title)
            };

            let edited_time = block.edit_time.max(page.latest_found_edit_time.get());

            let template_data = TemplateArgs {
                title: final_title.as_ref(),
                body: &rendered,
                tags,
                created_time: block.create_time,
                edited_time,
            };

            let template_key = page_templates
                .get(&page.id)
                .ok_or_else(|| eyre!("Failed to find template for page"))?;
            let full_page = handlebars.render(template_key, &template_data)?;

            let content_matches = match std::fs::read_to_string(&output_path) {
                Ok(existing) => existing == full_page,
                Err(_) => false,
            };

            if !content_matches {
                let mut writer = std::fs::File::create(&output_path)
                    .with_context(|| format!("Writing {}", output_path))?;
                writer.write_all(full_page.as_bytes())?;
                writer.flush()?;

                println!("Wrote: \"{final_title}\" to {slug}");
            }

            Ok::<_, eyre::Report>(Some((
                output_path,
                (
                    content_matches,
                    ManifestItem {
                        slug,
                        title: final_title.to_string(),
                        uid: block.uid.clone(),
                    },
                ),
            )))
        })
        .filter_map(|p| p.transpose())
        // Use BTreeMap since it gets us sorted keys in the output, which is good for
        // minimizing Git churn on the manifest.
        .collect::<Result<Vec<_>>>()?;

    let manifest_data = results
        .iter()
        .map(|(k, (_, manifest_item))| (k, manifest_item))
        .collect::<BTreeMap<_, _>>();

    let manifest_path = config.output.join("manifest.json");
    let mut manifest_writer = std::fs::File::create(&manifest_path)
        .with_context(|| format!("Writing {}", manifest_path.to_string_lossy()))?;
    serde_json::to_writer_pretty(&manifest_writer, &manifest_data)?;
    manifest_writer.flush()?;
    drop(manifest_writer);

    let skipped = results
        .iter()
        .filter(|(_, (content_matched, _))| *content_matched)
        .count();
    let wrote = results.len() - skipped;

    Ok((wrote, skipped))
}
