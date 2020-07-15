use crate::{annotation::AnnotationSet, Error};
use std::path::PathBuf;

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub enum SourceFile {
    Object(PathBuf),
}

impl SourceFile {
    pub fn annotations(&self) -> Result<AnnotationSet, Error> {
        let mut annotations = AnnotationSet::new();
        match self {
            Self::Object(file) => {
                let bytes = std::fs::read(file)?;
                crate::object::extract(&bytes, &mut annotations)?;
                Ok(annotations)
            }
        }
    }
}
