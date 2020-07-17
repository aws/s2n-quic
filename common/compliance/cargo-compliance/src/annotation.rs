use crate::{
    specification::Format,
    target::{Target, TargetSet},
    Error,
};
use core::{ops::Range, str::FromStr};
use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use triple_accel::levenshtein_search as text_search;

pub type AnnotationSet = BTreeSet<Annotation>;

pub type AnnotationReferenceMap<'a> =
    HashMap<(Target, Option<&'a str>), Vec<(usize, &'a Annotation)>>;

pub trait AnnotationSetExt {
    fn targets(&self) -> Result<TargetSet, Error>;
    fn reference_map(&self) -> Result<AnnotationReferenceMap, Error>;
}

impl AnnotationSetExt for AnnotationSet {
    fn targets(&self) -> Result<TargetSet, Error> {
        let mut set = TargetSet::new();
        for anno in self.iter() {
            set.insert(anno.target()?);
        }
        Ok(set)
    }

    fn reference_map(&self) -> Result<AnnotationReferenceMap, Error> {
        let mut map = AnnotationReferenceMap::new();
        for (id, anno) in self.iter().enumerate() {
            let target = anno.target()?;
            let section = anno.target_section();
            map.entry((target, section)).or_default().push((id, anno));
        }
        Ok(map)
    }
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Annotation {
    pub source: PathBuf,
    pub anno_line: u32,
    pub anno_column: u32,
    pub item_line: u32,
    pub item_column: u32,
    pub path: String,
    pub anno: AnnotationType,
    pub target: String,
    pub quote: String,
    pub code: String,
    pub manifest_dir: PathBuf,
    pub level: AnnotationLevel,
    pub format: Format,
}

impl Annotation {
    pub fn target(&self) -> Result<Target, Error> {
        Target::from_annotation(&self)
    }

    pub fn source(&self) -> Result<PathBuf, Error> {
        self.resolve_file(&self.source)
    }

    pub fn target_path(&self) -> &str {
        self.target.splitn(2, '#').next().unwrap()
    }

    pub fn target_section(&self) -> Option<&str> {
        self.target.splitn(2, '#').nth(1)
    }

    pub fn resolve_file(&self, file: &Path) -> Result<PathBuf, Error> {
        let mut manifest_dir = self.manifest_dir.clone();

        loop {
            if let Ok(file) = manifest_dir.join(&file).canonicalize() {
                return Ok(file);
            }

            if !manifest_dir.pop() {
                break;
            }
        }

        Err(format!("Could not resolve file {:?}", file).into())
    }

    pub fn quote_range(&self, contents: &str) -> Option<Range<usize>> {
        if self.quote.is_empty() {
            Some(0..contents.len())
        } else {
            text_search(self.quote.as_bytes(), contents.as_bytes())
                .find(|m| m.k < 2)
                .map(|m| m.start..m.end)
        }
    }

    #[allow(dead_code)]
    pub fn format_code(&mut self) -> Result<(), Error> {
        if self.code.is_empty() {
            return Ok(());
        }

        let mut child = Command::new("rustfmt")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        {
            let stdin = child.stdin.as_mut().expect("piped stdin above");
            stdin.write_all(self.code.as_bytes())?;
        }

        let output = child.wait_with_output()?;

        self.code = String::from_utf8(output.stdout)?;

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum AnnotationType {
    Spec,
    Test,
    Citation,
    Exception,
}

impl Default for AnnotationType {
    fn default() -> Self {
        Self::Citation
    }
}

impl FromStr for AnnotationType {
    type Err = Error;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        match v {
            "SPEC" => Ok(Self::Spec),
            "TEST" => Ok(Self::Test),
            "CITATION" => Ok(Self::Citation),
            "EXCEPTION" => Ok(Self::Exception),
            _ => Err(format!("Invalid annotation type {:?}", v).into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum AnnotationLevel {
    Auto,
    MUST,
    SHOULD,
    MAY,
}

impl Default for AnnotationLevel {
    fn default() -> Self {
        Self::Auto
    }
}

impl FromStr for AnnotationLevel {
    type Err = Error;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        match v {
            "AUTO" => Ok(Self::Auto),
            "MUST" => Ok(Self::MUST),
            "SHOULD" => Ok(Self::SHOULD),
            "MAY" => Ok(Self::MAY),
            _ => Err(format!("Invalid annotation level {:?}", v).into()),
        }
    }
}
