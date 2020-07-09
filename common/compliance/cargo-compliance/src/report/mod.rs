use crate::{
    annotation::{Annotation, AnnotationLevel, AnnotationSet, AnnotationSetExt},
    project::Project,
    source::Source,
    specification::Specification,
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
        let executables = self.project.executables()?;

        let annotations: AnnotationSet = executables
            .par_iter()
            .flat_map(|file| {
                let mut annotations = AnnotationSet::new();
                let bytes = std::fs::read(file).unwrap();
                crate::object::extract(&bytes, &mut annotations).unwrap();
                annotations
            })
            .collect();

        let sources = annotations.sources()?;

        let contents: HashMap<_, _> = sources
            .par_iter()
            .map(|source| {
                let contents = source.path.load().unwrap();
                (source, contents)
            })
            .collect();

        let specifications: HashMap<_, _> = contents
            .par_iter()
            .map(|(source, contents)| {
                let spec = source.format.parse(contents).unwrap();
                (source, spec)
            })
            .collect();

        let reference_map = annotations.reference_map()?;

        let results: Vec<_> = reference_map
            .par_iter()
            .flat_map(|((source, section_id), annotations)| {
                let spec = specifications.get(&source).expect("spec already checked");

                let mut results = vec![];

                if let Some(section_id) = section_id {
                    if let Some(section) = spec.sections.get(section_id) {
                        let contents = section.contents();

                        for (annotation_id, annotation) in annotations {
                            if let Some(range) = annotation.quote_range(&contents) {
                                for (line, range) in contents.ranges(range) {
                                    results.push(Ok((
                                        source,
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
                                    .push(Err((source, ReportError::QuoteMismatch { annotation })));
                            }
                        }
                    } else {
                        for (_, annotation) in annotations {
                            results.push(Err((source, ReportError::MissingSection { annotation })));
                        }
                    }
                } else {
                    // TODO
                    eprintln!("TOTAL REFERENCE");
                }

                // TODO upgrade levels whenever they overlap

                results
            })
            .collect();

        let mut report = ReportResult::default();

        for result in results {
            let (source, result) = match result {
                Ok((source, entry)) => (source, Ok(entry)),
                Err((source, err)) => (source, Err(err)),
            };

            let entry = report
                .sources
                .entry(&source)
                .or_insert_with(|| SourceReport {
                    errors: vec![],
                    source,
                    references: BTreeSet::new(),
                    contents: contents.get(&source).expect("content should exist"),
                    specification: specifications.get(&source).expect("content should exist"),
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
                .sources
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
    sources: HashMap<&'a Source, SourceReport<'a>>,
}

#[derive(Debug)]
pub struct SourceReport<'a> {
    errors: Vec<ReportError<'a>>,
    source: &'a Source,
    references: BTreeSet<Reference<'a>>,
    contents: &'a String,
    specification: &'a Specification<'a>,
}

impl<'a> SourceReport<'a> {
    #[allow(dead_code)]
    pub fn statistics(&self) -> Statistics {
        let mut stats = Statistics::default();

        for reference in &self.references {
            stats.record(reference);
        }

        stats
    }
}
