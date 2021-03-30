// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Reference;
use crate::annotation::{AnnotationLevel, AnnotationType};

#[derive(Clone, Copy, Debug, Default)]
pub struct Statistics {
    pub must: AnnotationStatistics,
    pub shall: AnnotationStatistics,
    pub should: AnnotationStatistics,
    pub may: AnnotationStatistics,
    pub recommended: AnnotationStatistics,
    pub optional: AnnotationStatistics,
}

impl Statistics {
    #[allow(dead_code)]
    pub(super) fn record(&mut self, reference: &Reference) {
        match reference.level {
            AnnotationLevel::Auto => {
                // don't record auto references
            }
            AnnotationLevel::Must => {
                self.must.record(reference);
            }
            AnnotationLevel::Shall => {
                self.shall.record(reference);
            }
            AnnotationLevel::Should => {
                self.should.record(reference);
            }
            AnnotationLevel::May => {
                self.may.record(reference);
            }
            AnnotationLevel::Optional => {
                self.optional.record(reference);
            }
            AnnotationLevel::Recommended => {
                self.recommended.record(reference);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AnnotationStatistics {
    pub total: Stat,
    pub citations: Stat,
    pub tests: Stat,
    pub exceptions: Stat,
    pub todos: Stat,
}

impl AnnotationStatistics {
    #[allow(dead_code)]
    fn record(&mut self, reference: &Reference) {
        self.total.record(reference);
        match reference.annotation.anno {
            AnnotationType::Citation => {
                self.citations.record(reference);
            }
            AnnotationType::Test => {
                self.tests.record(reference);
            }
            AnnotationType::Exception => {
                self.exceptions.record(reference);
            }
            AnnotationType::Todo => {
                self.todos.record(reference);
            }
            AnnotationType::Spec => {
                // do nothing, it's just a reference
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stat {
    pub range: u64,
    pub lines: u64,
    cursor: u64,
}

impl Stat {
    fn record(&mut self, reference: &Reference) {
        let start = reference.start as u64;
        let end = reference.end as u64;
        let len = end - start.max(self.cursor);
        if len > 0 {
            self.range += len;
            self.lines += 1;
        }
        self.cursor = end;
    }
}
