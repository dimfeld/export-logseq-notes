use std::{borrow::Cow, fs::File, io::Read, path::Path};

use ahash::HashMap;
use chrono::TimeZone;
use eyre::{Result, WrapErr};
use handlebars::*;
use itertools::Itertools;

handlebars_helper!(join: |list: array, sep: str| {
  list.iter().filter_map(|s| {
    match s {
      JsonValue::String(s) => Some(Cow::from(s)),
      JsonValue::Number(n) => Some(Cow::from(n.to_string())),
      _ => None,
    }
  }).join(sep)
});

handlebars_helper!(iso_time: |fmt:str, t: i64| { chrono::Utc.timestamp(t / 1000, 0).to_rfc3339() });
handlebars_helper!(format_time: |fmt:str, t: i64| { chrono::Utc.timestamp(t / 1000, 0).format(fmt).to_string() });
handlebars_helper!(replace: |content:str, pattern: str, replacement:str | content.replace(pattern, replacement) );

pub fn create(path: Option<&Path>) -> Result<Handlebars> {
    let mut hbars = handlebars::Handlebars::new();
    if let Some(path) = path {
        let mut template_file = match File::open(path).with_context(|| format!("{path:?}")) {
            Ok(f) => f,
            Err(e) => {
                if path.is_absolute() {
                    return Err(e);
                }

                // Try opening the file under `templates/{path}`. If that fails, return the
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

        hbars.register_template_string("default", template)?;
    }

    hbars.register_helper("join", Box::new(join));
    hbars.register_helper("format_time", Box::new(format_time));
    hbars.register_helper("replace", Box::new(replace));

    Ok(hbars)
}

pub struct DedupingTemplateRegistry<'a> {
    handlebars: Handlebars<'a>,
    templates: HashMap<String, String>,
    template_files: HashMap<String, String>,
}

impl<'a> DedupingTemplateRegistry<'a> {
    pub fn new(hbars: Handlebars<'a>) -> Self {
        DedupingTemplateRegistry {
            handlebars: hbars,
            templates: HashMap::default(),
            template_files: HashMap::default(),
        }
    }

    pub fn into_inner(self) -> Handlebars<'a> {
        self.handlebars
    }

    pub fn add_file_with_key(&mut self, key: String, path: &Path) -> Result<String> {
        let p = path.to_string_lossy();
        let existing = self.template_files.get(p.as_ref());
        if let Some(existing) = existing {
            return Ok(existing.clone());
        }

        let template =
            std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
        self.add_template(key, template)
    }

    pub fn add_file(&mut self, path: &Path) -> Result<String> {
        let key = format!("file:{}", path.display());
        self.add_file_with_key(key, path)
    }

    pub fn add_string(&mut self, value: String) -> Result<String> {
        let key = format!("value:{}", self.templates.len());
        self.add_template(key, value)
    }

    pub fn add_template(&mut self, key: String, template: String) -> Result<String> {
        let result_key = match self.templates.get(&template) {
            Some(r) => r.clone(),
            None => {
                self.handlebars.register_template_string(&key, &template)?;
                self.templates.insert(template, key.clone());
                key
            }
        };

        Ok(result_key)
    }
}
