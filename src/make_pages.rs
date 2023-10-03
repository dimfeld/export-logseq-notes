use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

use ahash::{HashMap, HashSet};
use eyre::{eyre, Result, WrapErr};
use itertools::Itertools;
use rayon::prelude::*;
use rhai::{packages::Package, Engine};
use serde::Serialize;

use crate::{
    config::{Config, PkmProduct},
    graph::{BlockInclude, Graph, ParsedPage},
    image::{image_full_path, Images},
    logseq::db::MetadataDb,
    page::{IdSlugUid, ManifestItem, Page, TitleSlugUid},
    parse_string::{ContentStyle, Expression},
    pic_store::PicStoreClient,
    script::{run_script_on_page, AllowEmbed, PageConfig, TemplateSelection},
    syntax_highlight,
};

#[derive(Serialize, Debug)]
struct TemplateArgs<'a> {
    title: &'a str,
    body: &'a str,
    tags: Vec<&'a str>,
    attrs: HashMap<&'a str, String>,
    created_time: u64,
    edited_time: u64,
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

struct ExtractedImage {
    path: PathBuf,
}

struct ExpressionContents {
    image_paths: Vec<ExtractedImage>,
    page_embeds: Vec<String>,
}

fn examine_expressions(
    contents: &mut ExpressionContents,
    base_path: &Path,
    page: &ParsedPage,
    expressions: &[Expression],
) {
    for expr in expressions {
        match expr {
            Expression::Image { url, .. } => {
                if let Some(path) = image_full_path(base_path, &page.path, url) {
                    contents.image_paths.push(ExtractedImage { path });
                }
            }
            Expression::PageEmbed(uid) => {
                contents.page_embeds.push(uid.to_string());
            }
            _ => {}
        }

        let contained = expr.contained_expressions();
        if !contained.is_empty() {
            examine_expressions(contents, base_path, page, contained);
        }
    }
}

fn examine_tags(
    contents: &mut ExpressionContents,
    base_path: &Path,
    page: &ParsedPage,
    block_index: usize,
) {
    let block = page.blocks.get(&block_index).unwrap();

    examine_expressions(contents, base_path, page, block.contents.borrow_parsed());

    for child in &block.children {
        examine_tags(contents, base_path, page, *child);
    }
}

struct ProcessedPage {
    config: PageConfig,
    blocks: ParsedPage,
    notable: ExpressionContents,
    heading_delta: isize,
    slug: String,
}

pub fn make_pages_from_script(
    pages: Vec<ParsedPage>,
    content_style: ContentStyle,
    explicit_ordering: bool,
    mut templates: crate::template::DedupingTemplateRegistry,
    highlighter: &syntax_highlight::Highlighter,
    global_config: &Config,
    metadata_db: Option<MetadataDb>,
) -> Result<(usize, usize)> {
    let package = crate::script::ParsePackage::new();
    let mut parse_engine = Engine::new_raw();
    package.register_into_engine(&mut parse_engine);

    let ast = parse_engine
        .compile_file(global_config.script.clone())
        .wrap_err("Parsing script")?;

    let base_dir = match global_config.product {
        PkmProduct::Logseq => global_config.path.canonicalize().unwrap(),
        PkmProduct::Roam => global_config.path.parent().unwrap().canonicalize().unwrap(),
    };

    let mut pages = pages
        .into_iter()
        .map(|parsed_page| {
            let (page_config, page_blocks) =
                run_script_on_page(&package, &ast, &global_config, parsed_page)
                    .wrap_err("Running script")?;

            let slug = create_path(
                page_config.url_base.as_str(),
                global_config.base_url.as_deref().unwrap_or(""),
                page_config.url_name.as_str(),
            );

            let mut notable = ExpressionContents {
                image_paths: Vec::new(),
                page_embeds: Vec::new(),
            };
            examine_tags(
                &mut notable,
                &base_dir,
                &page_blocks,
                page_blocks.root_block,
            );

            let h_element_delta = (page_config.top_header_level as isize) - 1;
            let markdown_heading_delta = if global_config.promote_headers {
                let lowest_heading = page_blocks
                    .blocks
                    .iter()
                    .filter(|(_, block)| {
                        if block.heading == 0 {
                            return false;
                        }

                        match block.include_type {
                            BlockInclude::JustBlock => true,
                            BlockInclude::AndChildren => true,
                            BlockInclude::OnlyChildren => false,
                            BlockInclude::Exclude => false,
                            BlockInclude::IfChildrenPresent => !block.children.is_empty(),
                        }
                    })
                    .map(|(_, block)| block.heading)
                    .min()
                    .unwrap_or(1) as isize;
                lowest_heading - 1
            } else {
                0
            };

            Ok::<_, eyre::Report>(ProcessedPage {
                config: page_config,
                heading_delta: h_element_delta - markdown_heading_delta,
                blocks: page_blocks,
                notable,
                slug,
            })
        })
        .filter(|result| match result {
            Ok(ProcessedPage { config, .. }) => {
                config.include || config.allow_embedding == AllowEmbed::Yes
            }
            _ => true,
        })
        .collect::<Result<Vec<_>>>()?;

    let embedded_pages = pages
        .iter()
        .flat_map(|page| page.notable.page_embeds.iter().map(|s| s.to_string()))
        .collect::<HashSet<_>>();

    // Sync the images with the CDN
    let image_info = if let Some(pc_config) = global_config.pic_store.as_ref() {
        let pc_client = PicStoreClient::new(pc_config)?;
        let images = Images::new(base_dir.to_path_buf(), pc_client, metadata_db.unwrap());

        let image_paths = pages
            .iter_mut()
            .filter(|ProcessedPage { config, blocks, .. }| {
                // The list of pages above includes not only explicitly included pages, but all
                // those that might be eligible for embedding. Here we want to filter that down to
                // just those that will actually be used in the output somewhere.
                if config.include {
                    return true;
                }

                let orig_title = blocks
                    .blocks
                    .get(&blocks.root_block)
                    .unwrap()
                    .page_title
                    .as_deref()
                    .unwrap_or("");

                embedded_pages.contains(orig_title)
            })
            .flat_map(
                |ProcessedPage {
                     config, notable, ..
                 }| {
                    notable
                        .image_paths
                        .drain(..)
                        .map(|path| (config.picture_upload_profile.as_deref(), path))
                },
            )
            .collect::<Vec<_>>();

        image_paths
            .into_par_iter()
            .try_for_each(|(profile_override, path)| images.add(path.path, profile_override))?;

        images.finish()?
    } else {
        HashMap::default()
    };

    let page_templates = pages
        .iter_mut()
        .map(|ProcessedPage { config, .. }| {
            let template_key = match std::mem::take(&mut config.template) {
                TemplateSelection::Default => {
                    if global_config.template.is_none() {
                        return Err(eyre!("Config has no default template, but page {} does not specify a template", config.title));
                    }
                    "default".to_string()
                },
                TemplateSelection::File(f) => templates.add_file(&PathBuf::from(f))?,
                TemplateSelection::Value(v) => templates.add_string(v)?,
            };

            let picture_template_key = match std::mem::take(&mut config.picture_template) {
                TemplateSelection::Default => {
                    "default_picture".to_string()
                }
                TemplateSelection::File(f) => templates.add_file(&PathBuf::from(f))?,
                TemplateSelection::Value(v) => templates.add_string(v)?,
            };

            Ok::<_, eyre::Report>((config.root_block, (template_key, picture_template_key)))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let pages_by_title = pages
        .iter()
        .map(
            |ProcessedPage {
                 config,
                 blocks,
                 slug,
                 ..
             }| {
                // Get the title from the page, not whatever the script might have changed it to, so that
                // links go to the correct place.
                let title = blocks
                    .blocks
                    .get(&config.root_block)
                    .unwrap()
                    .page_title
                    .as_deref()
                    .unwrap_or("")
                    .to_string();

                (
                    title,
                    IdSlugUid {
                        id: config.root_block,
                        output_title: config.title.clone(),
                        include: config.include,
                        allow_embed: match (config.include, config.allow_embedding) {
                            (true, AllowEmbed::Yes | AllowEmbed::Default) => true,
                            (true, AllowEmbed::No) => false,
                            (false, AllowEmbed::Yes) => true,
                            (false, AllowEmbed::No | AllowEmbed::Default) => false,
                        },
                        slug: slug.clone(),
                        uid: blocks.blocks.get(&config.root_block).unwrap().uid.clone(),
                    },
                )
            },
        )
        .collect::<HashMap<_, _>>();

    let pages_by_filename_title = pages
        .iter()
        .filter_map(|ProcessedPage { config, blocks, .. }| {
            let page_block = blocks.blocks.get(&config.root_block).unwrap();

            page_block
                .original_title
                .clone()
                .zip(page_block.page_title.clone())
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

    let default_output_dir = global_config.output.to_string_lossy();

    let omitted_attributes = global_config
        .omit_attributes
        .iter()
        .map(|x| x.as_str())
        .collect::<HashSet<_>>();

    let handlebars = templates.into_inner();

    let mut graph = Graph::new(content_style, explicit_ordering);
    for ProcessedPage { blocks, .. } in pages.iter_mut() {
        let blocks = std::mem::take(&mut blocks.blocks);
        for (_, block) in blocks {
            graph.add_block(block);
        }
    }

    let results = pages
        .into_par_iter()
        .map(
            |ProcessedPage {
                 config,
                 blocks,
                 slug,
                 heading_delta,
                 ..
             }| {
                if !config.include {
                    return Ok(None);
                }

                let filename = if config.path_name.is_empty() {
                    format!("{}.{}", config.url_name, global_config.extension)
                } else {
                    config.path_name
                };

                let output_path = create_path(
                    config.path_base.as_str(),
                    default_output_dir.as_ref(),
                    &filename,
                );

                let (template_key, picture_template_key) =
                    page_templates
                        .get(&config.root_block)
                        .ok_or_else(|| eyre!("Failed to find template for page"))?;

                let page = Page {
                    id: config.root_block,
                    title: config.title,
                    slug: slug.as_str(),
                    base_dir: &base_dir,
                    path: blocks.path,
                    latest_found_edit_time: std::cell::Cell::new(0),
                    graph: &graph,
                    config: global_config,
                    pages_by_title: &pages_by_title,
                    pages_by_filename_title: &pages_by_filename_title,
                    pages_by_id: &pages_by_id,
                    omitted_attributes: &omitted_attributes,
                    highlighter,
                    handlebars: &handlebars,
                    picture_template_key,
                    image_info: &image_info,
                    heading_delta,
                };

                let block = graph.blocks.get(&page.id).unwrap();

                let rendered = page.render()?;

                if rendered.is_empty() {
                    return Ok(None);
                }

                let mut tags = config.tags.iter().map(|s| s.as_str()).collect::<Vec<_>>();
                tags.sort_by_key(|k| k.to_lowercase());
                tags.dedup();

                // println!("{:?} {:?}", title, tags);

                let edited_time = block.edit_time.max(page.latest_found_edit_time.get());

                let template_attrs = config
                    .attrs
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.join(", ")))
                    .collect::<HashMap<_, _>>();

                let template_data = TemplateArgs {
                    title: page.title.as_str(),
                    body: &rendered,
                    tags,
                    attrs: template_attrs,
                    created_time: block.create_time,
                    edited_time,
                };

                let full_page = handlebars.render(template_key, &template_data)?;

                let content_matches = match std::fs::read_to_string(&output_path) {
                    Ok(existing) => existing == full_page,
                    Err(_) => false,
                };

                if !content_matches {
                    if page.config.safe_write {
                        let mut temp_out = tempfile::NamedTempFile::new()
                            .with_context(|| format!("Writing {output_path}"))?;
                        temp_out.write_all(full_page.as_bytes())?;
                        temp_out.flush()?;

                        let temp_path = temp_out.into_temp_path();
                        temp_path
                            .persist(&output_path)
                            .with_context(|| format!("Writing {output_path}"))?;
                    } else {
                        let mut writer = std::fs::File::create(&output_path)
                            .with_context(|| format!("Writing {output_path}"))?;
                        writer.write_all(full_page.as_bytes())?;
                        writer.flush()?;
                    }

                    println!("Wrote: \"{title}\" to {slug}", title = page.title);
                }

                Ok::<_, eyre::Report>(Some((
                    output_path,
                    (
                        content_matches,
                        ManifestItem {
                            title: page.title.to_string(),
                            slug,
                            uid: block.uid.clone(),
                        },
                    ),
                )))
            },
        )
        .filter_map(|p| p.transpose())
        // Use BTreeMap since it gets us sorted keys in the output, which is good for
        // minimizing Git churn on the manifest.
        .collect::<Result<Vec<_>>>()?;

    let manifest_data = results
        .iter()
        .map(|(k, (_, manifest_item))| (k, manifest_item))
        .collect::<BTreeMap<_, _>>();

    let manifest_path = global_config.output.join("manifest.json");
    let mut manifest_writer = std::fs::File::create(&manifest_path)
        .with_context(|| format!("Writing {}", manifest_path.display()))?;
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
