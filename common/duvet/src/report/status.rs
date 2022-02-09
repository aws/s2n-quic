// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Reference;
use crate::annotation::AnnotationType;
use core::ops::Deref;
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashSet};

// TODO use a real interval set
type IntervalSet<T> = HashSet<T>;

type AnnotationId = usize;

#[derive(Debug, Default)]
pub struct StatusMap(BTreeMap<AnnotationId, Spec>);

impl Deref for StatusMap {
    type Target = BTreeMap<AnnotationId, Spec>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl StatusMap {
    pub(super) fn populate(&mut self, references: &BTreeSet<Reference>) {
        let mut specs: BTreeMap<AnnotationId, Vec<&Reference>> = BTreeMap::new();
        let mut coverage: BTreeMap<usize, Vec<&Reference>> = BTreeMap::new();

        // first build up a map of all of the references at any given offset
        for r in references {
            if r.annotation.anno != AnnotationType::Spec {
                for offset in r.start..r.end {
                    coverage.entry(offset).or_default().push(r);
                }
            } else {
                specs.entry(r.annotation_id).or_default().push(r);
            }
        }

        self.0 = specs
            .par_iter()
            .map(|(anno_id, refs)| {
                let mut spec = SpecReport::default();
                for r in refs {
                    for offset in r.start..r.end {
                        spec.insert(offset, r);
                    }
                    for (offset, refs) in coverage.range(r.start..r.end) {
                        for r in refs {
                            spec.insert(*offset, r);
                            spec.related.insert(r.annotation_id);
                        }
                    }
                }
                (*anno_id, spec.finish())
            })
            .collect();
    }
}

#[derive(Debug, Default)]
pub struct Spec {
    pub spec: usize,
    pub incomplete: usize,
    pub citation: usize,
    pub test: usize,
    pub exception: usize,
    pub todo: usize,
    pub related: BTreeSet<AnnotationId>,
}

#[derive(Debug, Default)]
pub struct SpecReport {
    spec_offsets: IntervalSet<usize>,
    citation_offsets: IntervalSet<usize>,
    test_offsets: IntervalSet<usize>,
    exception_offsets: IntervalSet<usize>,
    todo_offsets: IntervalSet<usize>,
    related: BTreeSet<AnnotationId>,
}

impl SpecReport {
    fn insert(&mut self, offset: usize, reference: &Reference) {
        match reference.annotation.anno {
            AnnotationType::Spec => &mut self.spec_offsets,
            AnnotationType::Citation => &mut self.citation_offsets,
            AnnotationType::Test => &mut self.test_offsets,
            AnnotationType::Exception => &mut self.exception_offsets,
            AnnotationType::Todo => &mut self.todo_offsets,
        }
        .insert(offset);
    }

    fn finish(mut self) -> Spec {
        let spec = self.spec_offsets.len();

        // exceptions automatically mark the section as complete
        for offset in self.exception_offsets.iter() {
            self.spec_offsets.remove(offset);
        }

        // an offset needs to be both cited and tested to be complete
        for offset in self.citation_offsets.union(&self.test_offsets) {
            self.spec_offsets.remove(offset);
        }

        Spec {
            spec,
            incomplete: self.spec_offsets.len(),
            citation: self.citation_offsets.len(),
            test: self.test_offsets.len(),
            exception: self.exception_offsets.len(),
            todo: self.todo_offsets.len(),
            related: self.related,
        }
    }
}
