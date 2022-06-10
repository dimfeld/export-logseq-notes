use crate::config::Config;
use crate::graph::Graph;
use crate::page::{IdSlugUid, Page, TitleAndUid, TitleSlugUid};
use crate::syntax_highlight;
use anyhow::{Context, Result};
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use rayon::prelude::*;
use serde::Serialize;
use std::io::Write;

#[derive(Serialize, Debug)]
struct TemplateArgs<'a> {
    title: &'a str,
    body: &'a str,
    tags: Vec<&'a str>,
    created_time: usize,
    edited_time: usize,
}

fn title_to_slug(s: &str) -> String {
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

pub fn make_pages<'a, 'b>(
    graph: &'a Graph,
    handlebars: &handlebars::Handlebars,
    highlighter: &'b syntax_highlight::Highlighter,
    config: &'a Config,
) -> Result<FxHashMap<String, TitleAndUid>> {
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
        .collect::<FxHashSet<usize>>();

    let exclude_tag_names = config
        .exclude_tags
        .iter()
        .map(|s| s.as_str())
        .collect::<FxHashSet<_>>();

    let omitted_attributes = config
        .omit_attributes
        .iter()
        .map(|x| x.as_str())
        .collect::<FxHashSet<_>>();

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
        .collect::<FxHashMap<_, _>>();

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

            if (config.include_all && matches!(bool_include_attr, Some(false)))
                || (!config.include_all
                    && !block.tags.iter().any(|tag| all_filter_tags.contains(tag))
                    && !block
                        .attrs
                        .iter()
                        .any(|(attr_name, _)| all_filter_tags.contains(attr_name))
                    && !matches!(bool_include_attr, Some(true)))
                    && !all_filter_tags_regex.is_match(block.string.as_str())
            {
                // If we're including all pages, continue to exclude pages where the bool include
                // attribute is false.

                // If we're not including all pages, then check:
                // - The page tags to match the include tags
                // - The page attributes to match the include tags
                // - The boolean include attribute, if present
                // Return None if none of those match.
                return None;
            }

            // println!("Including block {block:?}");

            Some(block.containing_page)
        })
        .collect::<FxHashSet<_>>();

    let included_pages_by_title = included_page_ids
        .into_iter()
        .filter_map(|page_id| {
            let page = graph.blocks.get(&page_id)?;
            let title = page.page_title.as_ref()?;

            if title.starts_with("roam/") {
                // Don't include pages in roam/...
                return None;
            }

            if excluded_pages.contains(&page.id) || (page.is_journal && !config.allow_daily_notes) {
                // println!("Excluded: {}", page.title.as_ref().unwrap());
                return None;
            }

            // println!("Including {title}");

            let slug = pages_by_title.get(title).unwrap();
            Some((title.clone(), slug))
        })
        .collect::<FxHashMap<_, _>>();

    let included_pages_by_id = included_pages_by_title
        .iter()
        .map(|(title, IdSlugUid { id, slug, uid })| {
            (
                *id,
                TitleSlugUid {
                    title: title.clone(),
                    slug: slug.clone(),
                    uid: uid.clone(),
                },
            )
        })
        .collect::<FxHashMap<_, _>>();

    let filter_tags = [
        config.include.as_deref(),
        config.bool_include_attr.as_deref(),
    ]
    .iter()
    .filter_map(|v| *v)
    .collect::<Vec<_>>();

    let pages = included_pages_by_title
        .par_iter()
        .map(|(title, IdSlugUid { id, slug, uid })| {
            let mut output_path = config.output.join(slug);
            output_path.set_extension(&config.extension);

            let page = Page {
                id: *id,
                title: title.clone(),
                slug,
                graph,
                config,
                filter_tags: &filter_tags,
                pages_by_title: &pages_by_title,
                included_pages_by_title: &included_pages_by_title,
                included_pages_by_id: &included_pages_by_id,
                omitted_attributes: &omitted_attributes,
                highlighter,
            };

            let (rendered, hashtags) = page.render()?;

            let block = graph.blocks.get(id).unwrap();

            let tags_set = config
                .tags_attr
                .as_deref()
                .and_then(|tag_name| block.attrs.get(tag_name))
                .map(|values| values.iter().map(|s| s.as_str()).collect::<FxHashSet<_>>())
                .unwrap_or_else(FxHashSet::default);

            let hashtags = if config.use_all_hashtags {
                hashtags
            } else {
                FxHashSet::default()
            };

            let tags = tags_set
                .union(&hashtags)
                .copied()
                .filter(|&s| !filter_tags.contains(&s) && exclude_tag_names.get(s).is_none())
                .collect::<Vec<_>>();

            // println!("{:?} {:?}", title, tags);

            let template_data = TemplateArgs {
                title,
                body: &rendered,
                tags,
                created_time: block.create_time,
                edited_time: block.edit_time,
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
        .collect::<Result<FxHashMap<_, _>>>()?;

    let manifest_path = config.output.join("manifest.json");
    let mut manifest_writer = std::fs::File::create(&manifest_path)
        .with_context(|| format!("Writing {}", manifest_path.to_string_lossy()))?;
    serde_json::to_writer_pretty(&manifest_writer, &pages)?;
    manifest_writer.flush()?;
    drop(manifest_writer);

    Ok(pages)
}
