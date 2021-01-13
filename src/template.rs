use anyhow::{anyhow, Context as AnyhowContext, Result};
use chrono::TimeZone;
use handlebars::*;
use itertools::Itertools;
use std::borrow::Cow;
use std::fs::File;
use std::io::Read;
use std::path::Path;

handlebars_helper!(join: |list: array, sep: str| {
  list.iter().filter_map(|s| {
    match s {
      JsonValue::String(s) => Some(Cow::from(s)),
      JsonValue::Number(n) => Some(Cow::from(n.to_string())),
      _ => None,
    }
  }).join(sep)
});

handlebars_helper!(format_time: |fmt:str, t: i64| { chrono::Utc.timestamp(t / 1000, 0).format(fmt).to_string() });

pub fn create(path: &Path) -> Result<Handlebars> {
  let mut template_file = match File::open(path) {
    Ok(f) => f,
    Err(e) => {
      if path.is_absolute() {
        return Err(e.into());
      }

      // Try opening the file under `template/{path}`. If that fails, return the
      // original error.
      let template_dir_path = Path::new("templates").join(path);
      File::open(template_dir_path)
        .map_err(|_| e)
        .context("Opening template file")?
    }
  };

  let mut template = String::new();
  template_file.read_to_string(&mut template)?;
  drop(template_file);

  let mut hbars = handlebars::Handlebars::new();
  hbars.register_template_string("page", template)?;

  hbars.register_helper("join", Box::new(join));
  hbars.register_helper("format_time", Box::new(format_time));

  Ok(hbars)
}
