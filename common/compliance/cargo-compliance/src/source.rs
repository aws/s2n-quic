use crate::{annotation::Annotation, specification::Format, Error};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use url::Url;

pub type SourceSet = HashSet<Source>;

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct Source {
    pub path: SourcePath,
    pub format: Format,
}

impl Source {
    pub fn from_annotation(anno: &Annotation) -> Result<Self, Error> {
        let path = SourcePath::from_annotation(anno)?;
        Ok(Self {
            path,
            format: anno.format,
        })
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub enum SourcePath {
    Url(Url),
    Path(PathBuf),
}

impl SourcePath {
    pub fn from_annotation(anno: &Annotation) -> Result<Self, Error> {
        let spec = anno.spec();

        if spec.starts_with('/') {
            return Ok(Self::Path(spec.into()));
        }

        if spec.starts_with('.') {
            let path = anno.file()?.parent().unwrap().join(&spec);
            let path = path.canonicalize()?;
            return Ok(Self::Path(path));
        }

        if spec.contains("://") {
            let url = Url::parse(&spec)?;
            return Ok(Self::Url(url));
        }

        let path = anno.resolve_file(Path::new(&spec))?;
        Ok(Self::Path(path))
    }

    pub fn load(&self) -> Result<String, Error> {
        match self {
            Self::Url(_url) => todo!("urls are not implemented yet"),
            Self::Path(path) => {
                let mut contents = std::fs::read_to_string(path)?;
                if !contents.ends_with('\n') {
                    contents.push('\n');
                }
                Ok(contents)
            }
        }
    }

    pub fn local(&self) -> PathBuf {
        match self {
            Self::Url(_) => todo!("urls are not implemented yet"),
            Self::Path(path) => path.clone(),
        }
    }
}
