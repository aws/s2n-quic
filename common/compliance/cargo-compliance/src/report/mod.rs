use crate::{
    annotation::{Annotation, AnnotationLevel, AnnotationSet, AnnotationSetExt},
    project::Project,
    specification::Specification,
    target::Target,
    Error,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeSet, HashMap},
    io::BufWriter,
    path::PathBuf,
};
use structopt::StructOpt;

mod lcov;
mod stats;

use stats::Statistics;

#[derive(Debug, StructOpt)]
pub struct Report {
    #[structopt(flatten)]
    project: Project,

    #[structopt(long)]
    lcov: Option<PathBuf>,
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
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

impl Report {
    pub fn exec(&self) -> Result<(), Error> {
        let project_sources = self.project.sources()?;

        let annotations: AnnotationSet = project_sources
            .par_iter()
            .flat_map(|source| {
                // TODO gracefully handle error
                source.annotations().expect("could not extract annotations")
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

        let mut report = ReportResult::default();

        for result in results {
            let (target, result) = match result {
                Ok((target, entry)) => (target, Ok(entry)),
                Err((target, err)) => (target, Err(err)),
            };

            let entry = report
                .targets
                .entry(&target)
                .or_insert_with(|| TargetReport {
                    errors: vec![],
                    target,
                    references: BTreeSet::new(),
                    contents: contents.get(&target).expect("content should exist"),
                    specification: specifications.get(&target).expect("content should exist"),
                });

            match result {
                Ok(reference) => {
                    entry.references.insert(reference);
                }
                Err(err) => {
                    entry.errors.push(err);
                }
            }
        }

        if let Some(lcov_dir) = &self.lcov {
            std::fs::create_dir_all(&lcov_dir)?;
            let lcov_dir = lcov_dir.canonicalize()?;
            let results: Vec<Result<(), std::io::Error>> = report
                .targets
                .par_iter()
                .map(|(source, report)| {
                    let id = crate::fnv(source);
                    let path = lcov_dir.join(format!("compliance.{}.info", id));
                    let mut output = BufWriter::new(std::fs::File::create(&path)?);
                    lcov::report(report, &mut output)?;
                    Ok(())
                })
                .collect();

            for result in results {
                result?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct ReportResult<'a> {
    targets: HashMap<&'a Target, TargetReport<'a>>,
}

#[derive(Debug)]
pub struct TargetReport<'a> {
    errors: Vec<ReportError<'a>>,
    target: &'a Target,
    references: BTreeSet<Reference<'a>>,
    contents: &'a String,
    specification: &'a Specification<'a>,
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
