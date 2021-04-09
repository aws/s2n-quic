use super::{ReportResult, TargetReport};
use crate::annotation::AnnotationType;
use anyhow::anyhow;
use rayon::prelude::*;
use std::collections::HashSet;

pub fn report(report: &ReportResult) -> Result<(), anyhow::Error> {
    report
        .targets
        .par_iter()
        .map(|(_source, report)| enforce_source(report))
        .collect::<Result<(), anyhow::Error>>()
}

pub fn enforce_source(report: &TargetReport) -> Result<(), anyhow::Error> {
    let mut cited_lines = HashSet::new();
    let mut tested_lines = HashSet::new();
    let mut significant_lines = HashSet::new();

    // record all references to specific sections
    for reference in &report.references {
        let line = reference.line;

        significant_lines.insert(line);

        match reference.annotation.anno {
            AnnotationType::Test => {
                tested_lines.insert(line);
            }
            AnnotationType::Citation => {
                cited_lines.insert(line);
            }
            AnnotationType::Exception => {
                // mark exceptions as fully covered
                tested_lines.insert(line);
                cited_lines.insert(line);
            }
            AnnotationType::Spec | AnnotationType::Todo => {}
        }
    }

    if report.require_citations {
        // Significant lines are not cited.
        if significant_lines.difference(&cited_lines).next().is_some() {
            return Err(anyhow!("Specification requirements missing citation."));
        }
        // Citations that have no significance.
        if cited_lines.difference(&significant_lines).next().is_some() {
            return Err(anyhow!("Citation for non-existing specification."));
        }
    }

    if report.require_tests {
        // Cited lines without tests
        if cited_lines.difference(&tested_lines).next().is_some() {
            return Err(anyhow!("Citation missing test."));
        }

        // Tests without citation
        if cited_lines.difference(&tested_lines).next().is_some() {
            return Err(anyhow!("Test for non-existing citation."));
        }
    }

    Ok(())
}
