// mapping of word occurrence to the corresponding tag.
let autotags = #{
  "sql": "databases",
  "rust": "rust",
  "ml": "Machine Learning",
  "machine learning": "Machine Learning"
};

if page.is_journal {
  page.url_base = "/journals";
  page.path_base = "pkm-pages/journals";

  // The title has dashes instead of underscores, which makes for a nicer id.
  page.path_name = page.title + ".html";

  let saw_content_block = false;

  // Iterate over every block in the page, with a max depth of 1.
  // Depth starts at 0, which is the "page" block.
  each_block(1, |block, depth| {
    if depth == 0 {
      // The top level. All blocks will inherit from this view_type if they don't specify something else.
      block.view_type = ViewType::Document;
    } else if block.contents == "Journal" {
      // Include the Journal block, but skip rendering it and render its children in its place.
      block.include = BlockInclude::OnlyChildren;
      saw_content_block = true;

      // Scan the journal block and its children for tags.
      let tags = autotag(autotags, block.id);
      page.add_tags(tags);
    } else if block.contents == "Learning" ||
        block.contents == "Links" {
      // Render these as bulleted lists, if they aren't empty.
      block.view_type = ViewType::Bullet;
      block.heading = 3;
      block.include = BlockInclude::IfChildrenPresent;
      saw_content_block = true;

      let tags = autotag(autotags, block.id);
      page.add_tags(tags);
    } else if depth == 1 {
      // Exclude all other blocks from the pages.
      block.include = BlockInclude::Exclude;
    }
  });

  page.include = saw_content_block;
} else {
  page.url_base = "/notes";
  page.path_base = "pkm-pages/notes";

  let include = false;
  let exclude = false;

  // Handle Logseq "public" attribute
  if page.get_attr_first("public") == "true" {
    include = true;
  }

  // Allow changing the URL through an attribute like note-export:: a_new_url
  let new_slug = page.get_attr_first("note-export");
  if new_slug.len > 0 {
    page.path_name = new_slug + ".html";
  }

  // If these tags occur in any block, omit the page.
  let exclude_tags = [
    "Areas",
    "CV",
    "syntax-test"
  ];

  // Include pages that have these tags somewhere.
  let include_tags = [
    "learning",
    "svelte",
    "SQL",
    "database",
    "Mental Models",
    "JTBD",
    "note-export"
  ];

  each_block(9999, |block, depth| {
    let tags = block.tags;
    if !exclude && tags.some(|t| t in exclude_tags) {
      exclude = true;
    } else if !include && tags.some(|t| t in include_tags) {
      include = true;
    }

    // Add all the hashtags seen in this block.
    page.add_tags(tags);

    // Don't publish synced highlights
    if block.contents.starts_with("Highlights first synced") ||
          block.contents.starts_with("New highlights added") {
      block.include = BlockInclude::Exclude;
    }
  });

  if !exclude {
    // If we're not explicitly excluding this page, allow it to be embedded by other pages,
    // even if it didn't qualify to be included as its own page.
    page.allow_embedding = AllowEmbed::Yes;
  }

  let allow = include && !exclude;
  page.include = allow;
  if allow {
    // Handle namespaced pages, setting the final element as the title and adding the other elements as tags.
    let title = page.title.split("/");
    if title.len > 1 {
      page.title = title.pop();
      for t in title {
        if t == "Book" {
          page.add_tag("Books");
        } else if t == "Article" {
          page.add_tag("Articles")
        } else if t == "Project" {
          page.add_tag("Projects")
        }
      }
    }

    // Remove all these tags from the page.
    let omit_tags = [
      "Articles",
      "Inbox",
      "In Progress",
      "Newsletter Item",
      "Done",
      "Readwise",
      "Deliberate Practice",
      "Video",
      "note-export",
      "public",
      "progress",
      "Progress",
      "tags",
      "Tags"
    ];

    for tag in omit_tags {
      page.remove_tag(tag);
    }
  }
}

