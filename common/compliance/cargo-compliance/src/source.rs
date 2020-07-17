use crate::{annotation::AnnotationSet, pattern::Pattern, Error};
use std::path::PathBuf;

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub enum SourceFile<'a> {
    Object(PathBuf),
    Text(Pattern<'a>, PathBuf),
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
        }
    }
}
