use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Translator {
    language: String,
    messages: BTreeMap<String, String>,
}

impl Translator {
    pub fn load(paths: &AppPaths, language: &str) -> Result<Self, RuntimeError> {
        let language = match language {
            "zh_cn" => "zh_cn",
            _ => "en",
        };
        let path = [
            paths.config_dir.join("i18n").join(format!("{language}.json")),
            paths.root.join("config/i18n").join(format!("{language}.json")),
            PathBuf::from("config/i18n").join(format!("{language}.json")),
        ]
        .into_iter()
        .find(|path| path.is_file());
        let text = match path {
            Some(path) => std::fs::read_to_string(path)?,
            None if language == "zh_cn" => {
                include_str!("../../../../config/i18n/zh_cn.json").to_owned()
            }
            None => include_str!("../../../../config/i18n/en.json").to_owned(),
        };
        let messages = serde_json::from_str(&text)?;
        Ok(Self { language: language.into(), messages })
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn text(&self, key: &str) -> String {
        self.messages.get(key).cloned().unwrap_or_else(|| key.to_owned())
    }

    pub fn format(&self, key: &str, replacements: &[(&str, &str)]) -> String {
        let mut value = self.text(key).to_owned();
        for (name, replacement) in replacements {
            value = value.replace(&format!("{{{name}}}"), replacement);
        }
        value
    }
}
