use super::Reference;
use crate::annotation::{AnnotationLevel, AnnotationType};

#[derive(Clone, Copy, Debug, Default)]
pub struct Statistics {
    pub must: AnnotationStatistics,
    pub should: AnnotationStatistics,
    pub may: AnnotationStatistics,
}

impl Statistics {
    #[allow(dead_code)]
    pub(super) fn record(&mut self, reference: &Reference) {
        match reference.level {
            AnnotationLevel::MUST => {
                self.must.record(reference);
            }
            AnnotationLevel::SHOULD => {
                self.should.record(reference);
            }
            AnnotationLevel::MAY => {
                self.may.record(reference);
            }
            AnnotationLevel::Auto => {
                // TODO pull in default level
                self.may.record(reference);
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
