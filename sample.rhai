
if page.is_journal {
  page.url_base = "journals";
  page.path_base = "journals";

  let saw_content_block = false;
  page.each_block(1, |depth| {
    if depth == 0 {
      // The top level
      this.view_type = ViewType::Document;
    } else if this.contents == "Journal" {
      this.include = BlockInclude::OnlyChildren;
      saw_content_block = true;
      debug(this.contents);
    } else if this.contents == "Learning" ||
        this.contents == "Links" {
      this.view_type = ViewType::Bullet;
      this.heading = 3;
      this.include = BlockInclude::IfChildrenPresent;
      saw_content_block = true;
      debug(this.contents);
    } else if depth > 0 {
      this.include = BlockInclude::Exclude;
    }
  });

  if saw_content_block {
    debug(page.title);
  }
  page.allow_render(saw_content_block);
} else {
  page.url_base = "notes";
  page.path_base = "pages";

  let include = false;
  let exclude = false;

  if page.get_attr_first("public") == "true" {
    include = true;
  }

  let exclude_tags = [
    "Areas",
    "CV"
  ];

  let include_tags = [
    "UI",
    "syntax-test"
  ];

  page.each_block(9999, |depth| {
    let tags = this.tags;
    if !exclude && tags.some(|t| t in exclude_tags) {
      exclude = true;
    } else if !include && tags.some(|t| t in include_tags) {
      include = true;
    }
  });

  page.allow_render(include && !exclude);
}
