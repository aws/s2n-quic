use crate::{
    annotation::{Annotation, AnnotationLevel, AnnotationSet, AnnotationType},
    pattern::Pattern,
    specification::Format,
    Error,
};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub enum SourceFile<'a> {
    Object(PathBuf),
    Text(Pattern<'a>, PathBuf),
    Spec(PathBuf),
}

impl<'a> SourceFile<'a> {
    pub fn annotations(&self) -> Result<AnnotationSet, Error> {
        let mut annotations = AnnotationSet::new();
        match self {
            Self::Object(file) => {
                let bytes = std::fs::read(file)?;
                crate::object::extract(&bytes, &mut annotations)?;
                Ok(annotations)
            }
            Self::Text(pattern, file) => {
                let text = std::fs::read_to_string(file)?;
                pattern.extract(&text, &file, &mut annotations)?;
                Ok(annotations)
            }
            Self::Spec(file) => {
                let text = std::fs::read_to_string(&file)?;
                for spec in toml::from_str::<Specs>(&text)?.specs {
                    annotations.insert(spec.into_annotation(file.clone())?);
                }
                Ok(annotations)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Specs<'a> {
    #[serde(borrow)]
    #[serde(alias = "spec", default)]
    specs: Vec<Spec<'a>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec<'a> {
    target: String,
    level: Option<&'a str>,
    format: Option<&'a str>,
    quote: &'a str,
}

impl<'a> Spec<'a> {
    fn into_annotation(self, source: PathBuf) -> Result<Annotation, Error> {
        Ok(Annotation {
            anno_line: 1,
            anno_column: 0,
            item_line: 1,
            item_column: 0,
            path: String::new(),
            anno: AnnotationType::Spec,
            target: self.target,
            quote: self.quote.trim().replace('\n', " "),
            code: String::new(),
            manifest_dir: source.clone(),
            source,
            level: if let Some(level) = self.level {
                level.parse()?
            } else {
                AnnotationLevel::Auto
            },
            format: if let Some(format) = self.format {
                format.parse()?
            } else {
                Format::Auto
            },
        })
    }
}
