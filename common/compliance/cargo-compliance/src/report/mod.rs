// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    annotation::{Annotation, AnnotationLevel, AnnotationSet, AnnotationSetExt},
    project::Project,
    specification::Specification,
    target::Target,
    Error,
};
use anyhow::anyhow;
use core::fmt;
use rayon::prelude::*;
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};
use structopt::StructOpt;

mod ci;
mod html;
mod json;
mod lcov;
mod stats;
mod status;

use stats::Statistics;

#[derive(Debug, StructOpt)]
pub struct Report {
    #[structopt(flatten)]
    project: Project,

    #[structopt(long)]
    lcov: Option<PathBuf>,

    #[structopt(long)]
    json: Option<PathBuf>,

    #[structopt(long)]
    html: Option<PathBuf>,

    #[structopt(long)]
    require_citations: Option<Option<bool>>,

    #[structopt(long)]
    require_tests: Option<Option<bool>>,

    #[structopt(long)]
    ci: bool,

    #[structopt(long)]
    blob_link: Option<String>,

    #[structopt(long)]
    issue_link: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord)]
struct Reference<'a> {
    line: usize,
    start: usize,
    end: usize,
    annotation_id: usize,
    annotation: &'a Annotation,
    level: AnnotationLevel,
}

#[derive(Debug)]
enum ReportError<'a> {
    QuoteMismatch { annotation: &'a Annotation },
    MissingSection { annotation: &'a Annotation },
}

impl<'a> fmt::Display for ReportError<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::QuoteMismatch { annotation } => write!(
                f,
                "{}#{}:{} - quote not found in {:?}",
                annotation.source.display(),
                annotation.anno_line,
                annotation.anno_column,
                annotation.target,
            ),
            Self::MissingSection { annotation } => write!(
                f,
                "{}#{}:{} - section {:?} not found in {:?}",
                annotation.source.display(),
                annotation.anno_line,
                annotation.anno_column,
                annotation.target_section().unwrap_or("-"),
                annotation.target_path(),
            ),
        }
    }
}

impl Report {
    pub fn exec(&self) -> Result<(), Error> {
        let project_sources = self.project.sources()?;

        let annotations: AnnotationSet = project_sources
            .par_iter()
            .flat_map(|source| {
                // TODO gracefully handle error
                source
                    .annotations()
                    .unwrap_or_else(|_| panic!("could not extract annotations from {:?}", source))
            })
            .collect();

        let targets = annotations.targets()?;

        let contents: HashMap<_, _> = targets
            .par_iter()
            .map(|target| {
                let contents = target.path.load().unwrap();
                (target, contents)
            })
            .collect();

        let specifications: HashMap<_, _> = contents
            .par_iter()
            .map(|(target, contents)| {
                let spec = target.format.parse(contents).unwrap();
                (target, spec)
            })
            .collect();

        let reference_map = annotations.reference_map()?;

        let results: Vec<_> = reference_map
            .par_iter()
            .flat_map(|((target, section_id), annotations)| {
                let spec = specifications.get(&target).expect("spec already checked");

                let mut results = vec![];

                if let Some(section_id) = section_id {
                    if let Some(section) = spec.sections.get(section_id) {
                        let contents = section.contents();

                        for (annotation_id, annotation) in annotations {
                            if annotation.quote.is_empty() {
                                // empty quotes don't count towards coverage but are still
                                // references
                                let title = section.title;
                                let range = title.range();
                                results.push(Ok((
                                    target,
                                    Reference {
                                        line: title.line,
                                        start: range.start,
                                        end: range.end,
                                        annotation,
                                        annotation_id: *annotation_id,
                                        level: annotation.level,
                                    },
                                )));
                                continue;
                            }

                            if let Some(range) = annotation.quote_range(&contents) {
                                for (line, range) in contents.ranges(range) {
                                    results.push(Ok((
                                        target,
                                        Reference {
                                            line,
                                            start: range.start,
                                            end: range.end,
                                            annotation,
                                            annotation_id: *annotation_id,
                                            level: annotation.level,
                                        },
                                    )));
                                }
                            } else {
                                results
                                    .push(Err((target, ReportError::QuoteMismatch { annotation })));
                            }
                        }
                    } else {
                        for (_, annotation) in annotations {
                            results.push(Err((target, ReportError::MissingSection { annotation })));
                        }
                    }
                } else {
                    // TODO
                    eprintln!("TOTAL REFERENCE {:?}", annotations);
                }

                // TODO upgrade levels whenever they overlap

                results
            })
            .collect();

        let mut report = ReportResult {
            targets: Default::default(),
            annotations: &annotations,
            blob_link: self.blob_link.as_deref(),
            issue_link: self.issue_link.as_deref(),
        };
        let mut errors = BTreeSet::new();

        for result in results {
            let (target, result) = match result {
                Ok((target, entry)) => (target, Ok(entry)),
                Err((target, err)) => (target, Err(err)),
            };

            let entry = report
                .targets
                .entry(target)
                .or_insert_with(|| TargetReport {
                    target,
                    references: BTreeSet::new(),
                    contents: contents.get(&target).expect("content should exist"),
                    specification: specifications.get(&target).expect("content should exist"),
                    require_citations: self.require_citations(),
                    require_tests: self.require_tests(),
                    statuses: Default::default(),
                });

            match result {
                Ok(reference) => {
                    entry.references.insert(reference);
                }
                Err(err) => {
                    errors.insert(err.to_string());
                }
            }
        }

        if !errors.is_empty() {
            for error in &errors {
                eprintln!("{}", error);
            }

            return Err(anyhow!(
                "source errors were found. no reports were generated"
            ));
        }

        report
            .targets
            .par_iter_mut()
            .for_each(|(_, target)| target.statuses.populate(&target.references));

        if let Some(dir) = &self.lcov {
            lcov::report(&report, dir)?;
        }

        if let Some(file) = &self.json {
            json::report(&report, file)?;
        }

        if let Some(dir) = &self.html {
            html::report(&report, dir)?;
        }

        if self.ci {
            ci::report(&report)?;
        }

        Ok(())
    }

    fn require_citations(&self) -> bool {
        match self.require_citations {
            None => true,
            Some(None) => true,
            Some(Some(value)) => value,
        }
    }

    fn require_tests(&self) -> bool {
        match self.require_tests {
            None => true,
            Some(None) => true,
            Some(Some(value)) => value,
        }
    }
}

#[derive(Debug)]
pub struct ReportResult<'a> {
    pub targets: HashMap<&'a Target, TargetReport<'a>>,
    pub annotations: &'a AnnotationSet,
    pub blob_link: Option<&'a str>,
    pub issue_link: Option<&'a str>,
}

#[derive(Debug)]
pub struct TargetReport<'a> {
    target: &'a Target,
    references: BTreeSet<Reference<'a>>,
    contents: &'a str,
    specification: &'a Specification<'a>,
    require_citations: bool,
    require_tests: bool,
    statuses: status::StatusMap,
}

impl<'a> TargetReport<'a> {
    #[allow(dead_code)]
    pub fn statistics(&self) -> Statistics {
        let mut stats = Statistics::default();

        for reference in &self.references {
            stats.record(reference);
        }

        stats
    }
}
