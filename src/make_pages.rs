use crate::config::Config;
use crate::graph::Graph;
use crate::page::{IdSlugUid, IncludeScope, Page, TitleAndUid, TitleSlugUid};
use crate::script::{run_script_on_page, AllowEmbed, PageConfig, TemplateSelection};
use crate::syntax_highlight;
use ahash::{HashMap, HashSet};
use eyre::{eyre, Result, WrapErr};
use itertools::Itertools;
use rayon::prelude::*;
use rhai::packages::Package;
use rhai::Engine;
use serde::Serialize;
use smallvec::{smallvec, SmallVec};
use std::borrow::Cow;
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
                .filter(|c| c.is_alphabetic() || c.is_digit(10))
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
    mut graph: Graph,
    mut templates: crate::template::DedupingTemplateRegistry,
    highlighter: &syntax_highlight::Highlighter,
    config: &Config,
) -> Result<usize> {
    let page_list = std::mem::take(&mut graph.page_blocks);
    let graph = Arc::new(std::sync::Mutex::new(graph));
    let package = crate::script::ParsePackage::new();
    let mut parse_engine = Engine::new_raw();
    package.register_into_engine(&mut parse_engine);

    let ast = parse_engine
        .compile_file(config.script.clone())
        .map_err(|e| eyre!("{e:?}"))
        .wrap_err("Parsing script")?;

    let mut pages = page_list
        .par_iter()
        .map(|block_id| {
            let mut engine = Engine::new_raw();

            engine.on_print(|x| println!("script: {x}"));
            engine.on_debug(|x, _src, pos| {
                println!("script:{pos:?}: {x}");
            });

            package.register_into_engine(&mut engine);

            let page_config = run_script_on_page(&mut engine, &ast, &graph, *block_id)
                .wrap_err("Running script")?;
            let slug = create_path(
                page_config.url_base.as_str(),
                config.base_url.as_deref().unwrap_or(""),
                page_config.url_name.as_str(),
            );

            Ok::<_, eyre::Report>((page_config, slug))
        })
        .filter(|result| match result {
            Ok((page, _)) => page.include || page.allow_embedding == AllowEmbed::Yes,
            _ => true,
        })
        .collect::<Result<Vec<_>>>()?;

    let page_templates = pages
        .iter_mut()
        .map(|(page, _)| {
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

    let graph = Arc::try_unwrap(graph)
        .expect("Pulling graph out or Arc")
        .into_inner()
        .expect("Pulling graph out of mutex");
    let pages_by_title = pages
        .iter()
        .map(|(p, slug)| {
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
                    uid: graph.blocks.get(&p.root_block).unwrap().uid.clone(),
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

    let results = pages
        .into_par_iter()
        .map(|(page_config, slug)| {
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

            let hashtags = if config.use_all_hashtags {
                hashtags
            } else {
                HashSet::default()
            };

            let tags_set = config
                .tags_attr
                .as_deref()
                .and_then(|tag_name| block.attrs.get(tag_name))
                .map(|values| values.iter().map(|s| s.as_str()).collect::<HashSet<_>>())
                .unwrap_or_else(HashSet::default);

            let tags = tags_set
                .union(&hashtags)
                .copied()
                .filter(|&s| omitted_attributes.get(s).is_none())
                .collect::<Vec<_>>();

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

            let mut writer = std::fs::File::create(&output_path)
                .with_context(|| format!("Writing {}", output_path))?;
            writer.write_all(full_page.as_bytes())?;
            writer.flush()?;

            println!("Wrote: \"{final_title}\" to {slug}");

            Ok::<_, eyre::Report>(Some((
                output_path,
                TitleAndUid {
                    title: final_title.to_string(),
                    uid: block.uid.clone(),
                },
            )))
        })
        .filter_map(|p| p.transpose())
        .collect::<Result<HashMap<_, _>>>()?;

    let manifest_path = config.output.join("manifest.json");
    let mut manifest_writer = std::fs::File::create(&manifest_path)
        .with_context(|| format!("Writing {}", manifest_path.to_string_lossy()))?;
    serde_json::to_writer_pretty(&manifest_writer, &results)?;
    manifest_writer.flush()?;
    drop(manifest_writer);

    Ok(results.len())
}

/*
pub fn make_pages<'a, 'b>(
    graph: &'a Graph,
    handlebars: &handlebars::Handlebars,
    highlighter: &'b syntax_highlight::Highlighter,
    config: &'a Config,
) -> Result<HashMap<String, TitleAndUid>> {
    let mut all_filter_tags = Vec::new();
    if let Some(include) = config.include.clone() {
        all_filter_tags.push(include);
    }
    all_filter_tags.extend_from_slice(&config.also);

    let all_filter_tags_for_regex = all_filter_tags.iter().map(|s| regex::escape(s)).join("|");
    let all_filter_tags_regex = regex::Regex::new(&format!(
        r##"(^|\s)#({})($|\s)"##,
        all_filter_tags_for_regex
    ))?;

    let block_include_prefix_for_regex = config
        .include_blocks_with_prefix
        .iter()
        .map(|s| regex::escape(s.as_str()))
        .join("|");
    let block_include_prefix_regex = (!block_include_prefix_for_regex.is_empty())
        .then(|| regex::Regex::new(&format!(r##"^({})($|\s)"##, block_include_prefix_for_regex)))
        .transpose()?;

    let block_include_tags_for_regex = config
        .include_blocks_with_tags
        .iter()
        .map(|s| regex::escape(s.as_str()))
        .join("|");
    let block_include_tags_regex = (!block_include_tags_for_regex.is_empty())
        .then(|| {
            regex::Regex::new(&format!(
                r##"(^|\s)#({})($|\s)"##,
                block_include_tags_for_regex
            ))
        })
        .transpose()?;

    let exclude_page_tags = config
        .exclude
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>();

    let excluded_pages = graph
        .blocks_with_references(&exclude_page_tags)
        .map(|block| block.containing_page)
        .chain(
            exclude_page_tags
                .iter()
                .filter_map(|tag| graph.titles.get(*tag))
                .copied(),
        )
        .collect::<HashSet<usize>>();

    let exclude_tag_names = config
        .exclude_tags
        .iter()
        .map(|s| s.as_str())
        .collect::<HashSet<_>>();

    let omitted_attributes = config
        .omit_attributes
        .iter()
        .map(|x| x.as_str())
        .collect::<HashSet<_>>();

    let pages_by_title = graph
        .pages()
        .map(|page| {
            let slug = config
                .include
                .as_ref()
                .and_then(|include_attr| page.attrs.get(include_attr).map(|v| &v[0]).cloned())
                .unwrap_or_else(|| title_to_slug(page.page_title.as_ref().unwrap()));

            (
                page.page_title.clone().unwrap(),
                IdSlugUid {
                    id: page.id,
                    slug,
                    uid: page.uid.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let included_page_ids = graph
        .blocks
        .iter()
        .filter_map(|(_, block)| {
            let bool_include_attr = config
                .bool_include_attr
                .as_ref()
                .and_then(|attr_name| block.attrs.get(attr_name))
                .map(|v| v[0] == "true");

            // if block.tags.len() > 0 {
            //     println!("{:?}", block.tags);
            // }

            // We're including all pages, or this page is explicitly included.
            if config.include_all || matches!(bool_include_attr, Some(true)) {
                return Some((block.containing_page, None));
            }

            // Look at all the cases that can include the entire page, if this page isn't
            // explicitly not included.
            if !(config.include_all && matches!(bool_include_attr, Some(false))) {
                // - The page tags to match the include tags
                // - The page attributes to match the include tags
                // - The boolean include attribute, if present
                if block.tags.iter().any(|tag| all_filter_tags.contains(tag))
                    || block
                        .attrs
                        .iter()
                        .any(|(attr_name, _)| all_filter_tags.contains(attr_name))
                    || all_filter_tags_regex.is_match(block.string.as_str())
                {
                    return Some((block.containing_page, None));
                }
            }

            // Look at if we're including just this block. These checks only apply to top-level
            // blocks.
            if !block.children.is_empty()
                && block
                    .parent
                    .as_ref()
                    .map(|p| p == &block.containing_page)
                    .unwrap_or(false)
            {
                if let Some(re) = block_include_tags_regex.as_ref() {
                    if re.is_match(block.string.as_str()) {
                        return Some((block.containing_page, Some(block.id)));
                    }
                }

                if let Some(re) = block_include_prefix_regex.as_ref() {
                    if re.is_match(block.string.as_str()) {
                        return Some((block.containing_page, Some(block.id)));
                    }
                }
            }

            // Return None if none of the above match.
            None
        })
        .fold(HashMap::default(), |mut acc, (page, specific_block)| {
            acc.entry(page)
                .and_modify(|mut e| {
                    match (&mut e, specific_block) {
                        // It's already full, nothing to do.
                        (IncludeScope::Full, _) => {}
                        (IncludeScope::Partial(_), None) => {
                            *e = IncludeScope::Full;
                        }
                        (IncludeScope::Partial(p), Some(b)) => {
                            p.push(b);
                        }
                    }
                })
                .or_insert_with(|| match specific_block {
                    None => IncludeScope::Full,
                    Some(b) => IncludeScope::Partial(smallvec![b]),
                });
            acc
        });

    let included_pages_by_title = included_page_ids
        .into_iter()
        .filter_map(|(page_id, include_scope)| {
            let page = graph.blocks.get(&page_id)?;
            let title = page.page_title.as_ref()?;

            if title.starts_with("roam/") {
                // Don't include pages in roam/...
                return None;
            }

            if excluded_pages.contains(&page.id)
                || (page.is_journal && config.daily_notes == DailyNotes::Deny)
                || (!page.is_journal && config.daily_notes == DailyNotes::Exclusive)
            {
                // println!("Excluded: {}", page.title.as_ref().unwrap());
                return None;
            }

            // println!("Including {title}");

            let slug = pages_by_title.get(title).unwrap();
            Some((title.clone(), (slug, include_scope)))
        })
        .collect::<HashMap<_, _>>();

    let included_pages_by_id = included_pages_by_title
        .iter()
        .map(|(title, (IdSlugUid { id, slug, uid }, _))| {
            (
                *id,
                TitleSlugUid {
                    title: title.clone(),
                    slug: slug.clone(),
                    uid: uid.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let filter_tags = [
        config.include.as_deref(),
        config.bool_include_attr.as_deref(),
    ]
    .iter()
    .filter_map(|v| *v)
    .collect::<Vec<_>>();

    let pages = included_pages_by_title
        .par_iter()
        .map(|(title, (IdSlugUid { id, slug, uid }, include_scope))| {
            let mut output_path = config.output.join(slug);
            output_path.set_extension(&config.extension);

            let page = Page {
                id: *id,
                title: title.clone(),
                slug,
                latest_found_edit_time: std::cell::Cell::new(0),
                graph,
                config,
                filter_tags: &filter_tags,
                pages_by_title: &pages_by_title,
                include_scope,
                included_pages_by_title: &included_pages_by_title,
                included_pages_by_id: &included_pages_by_id,
                omitted_attributes: &omitted_attributes,
                include_blocks_with_tags: &config.include_blocks_with_tags,
                include_blocks_with_prefix: &config.include_blocks_with_prefix,
                highlighter,
            };

            let (rendered, hashtags) = page.render()?;

            let block = graph.blocks.get(id).unwrap();

            let tags_set = config
                .tags_attr
                .as_deref()
                .and_then(|tag_name| block.attrs.get(tag_name))
                .map(|values| values.iter().map(|s| s.as_str()).collect::<HashSet<_>>())
                .unwrap_or_else(HashSet::default);

            let hashtags = if config.use_all_hashtags {
                hashtags
            } else {
                HashSet::default()
            };

            let tags = tags_set
                .union(&hashtags)
                .copied()
                .filter(|&s| !filter_tags.contains(&s) && exclude_tag_names.get(s).is_none())
                .collect::<Vec<_>>();

            let lower_tags = tags
                .iter()
                .map(|t| t.to_lowercase())
                .collect::<HashSet<_>>();

            // println!("{:?} {:?}", title, tags);

            let edited_time = block.edit_time.max(page.latest_found_edit_time.get());

            let mut popped = false;
            let mut title_components = title.split('/').collect::<SmallVec<[&str; 3]>>();
            while title_components.len() > 1
                && lower_tags.contains(&title_components[0].to_lowercase())
            {
                popped = true;
                title_components.remove(0);
            }

            let final_title = if popped {
                Cow::from(title_components.iter().join("/"))
            } else {
                Cow::from(title)
            };

            let template_data = TemplateArgs {
                title: final_title.as_ref(),
                body: &rendered,
                tags,
                created_time: block.create_time,
                edited_time,
            };
            let full_page = handlebars.render("page", &template_data)?;

            let mut writer = std::fs::File::create(&output_path)
                .with_context(|| format!("Writing {}", output_path.to_string_lossy()))?;
            writer.write_all(full_page.as_bytes())?;
            writer.flush()?;

            println!("Wrote: \"{}\" to {}", title, slug);

            Ok((
                slug.clone(),
                TitleAndUid {
                    title: title.clone(),
                    uid: uid.clone(),
                },
            ))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let manifest_path = config.output.join("manifest.json");
    let mut manifest_writer = std::fs::File::create(&manifest_path)
        .with_context(|| format!("Writing {}", manifest_path.to_string_lossy()))?;
    serde_json::to_writer_pretty(&manifest_writer, &pages)?;
    manifest_writer.flush()?;
    drop(manifest_writer);

    Ok(pages)
}
*/
