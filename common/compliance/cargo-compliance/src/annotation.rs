// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    specification::Format,
    target::{Target, TargetSet},
    Error,
};
use anyhow::anyhow;
use core::{fmt, ops::Range, str::FromStr};
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
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
    pub comment: String,
    pub manifest_dir: PathBuf,
    pub level: AnnotationLevel,
    pub format: Format,
    pub tracking_issue: String,
    pub feature: String,
    pub tags: BTreeSet<String>,
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

    // The JSON file needs to index the specification
    // to the same path that the annotation targets will have
    pub fn resolve_target_path(&self) -> String {
        let target_path = self.target_path();
        match target_path.contains("://") {
            // A URL should not be changed.
            true => String::from(target_path),
            // A file path needs to match
            false => String::from(
                self.resolve_file(Path::new(target_path))
                    .unwrap()
                    .to_str()
                    .unwrap(),
            ),
        }
    }

    pub fn target_section(&self) -> Option<&str> {
        self.target.splitn(2, '#').nth(1).map(|section| {
            // allow references to specify a #section-123 instead of #123
            section
                .trim_start_matches("section-")
                .trim_start_matches("appendix-")
        })
    }

    pub fn resolve_file(&self, file: &Path) -> Result<PathBuf, Error> {
        let mut manifest_dir = self.manifest_dir.clone();
        loop {
            if manifest_dir.join(&file).is_file() {
                return Ok(manifest_dir.join(&file));
            }

            if !manifest_dir.pop() {
                break;
            }
        }

        Err(anyhow!(format!("Could not resolve file {:?}", file)))
    }

    pub fn quote_range(&self, contents: &str) -> Option<Range<usize>> {
        if self.quote.is_empty() {
            // Don't actually consider full-section quotes as valid
            None
        } else {
            text_search(self.quote.as_bytes(), contents.as_bytes())
                .find(|m| m.k < 2)
                .map(|m| m.start..m.end)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum AnnotationType {
    Spec,
    Test,
    Citation,
    Exception,
    Todo,
}

impl Default for AnnotationType {
    fn default() -> Self {
        Self::Citation
    }
}

impl fmt::Display for AnnotationType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Self::Spec => "SPEC",
            Self::Test => "TEST",
            Self::Citation => "CITATION",
            Self::Exception => "EXCEPTION",
            Self::Todo => "TODO",
        })
    }
}

impl FromStr for AnnotationType {
    type Err = Error;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        match v {
            "SPEC" | "spec" => Ok(Self::Spec),
            "TEST" | "test" => Ok(Self::Test),
            "CITATION" | "citation" => Ok(Self::Citation),
            "EXCEPTION" | "exception" => Ok(Self::Exception),
            "TODO" | "todo" => Ok(Self::Todo),
            _ => Err(anyhow!(format!("Invalid annotation type {:?}", v))),
        }
    }
}

// The order is in terms of priority from least to greatest
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub enum AnnotationLevel {
    Auto,
    May,
    Should,
    Must,
}

impl AnnotationLevel {
    pub const LEVELS: [Self; 4] = [Self::Auto, Self::May, Self::Should, Self::Must];
}

impl Default for AnnotationLevel {
    fn default() -> Self {
        Self::Auto
    }
}

impl fmt::Display for AnnotationLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "AUTO",
            Self::May => "MAY",
            Self::Should => "SHOULD",
            Self::Must => "MUST",
        })
    }
}

impl FromStr for AnnotationLevel {
    type Err = Error;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        match v {
            "AUTO" => Ok(Self::Auto),
            "MUST" => Ok(Self::Must),
            "SHOULD" => Ok(Self::Should),
            "MAY" => Ok(Self::May),
            _ => Err(anyhow!(format!("Invalid annotation level {:?}", v))),
        }
    }
}
