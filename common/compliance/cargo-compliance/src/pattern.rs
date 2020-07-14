use crate::{
    annotation::{Annotation, AnnotationSet},
    parser::ParsedAnnotation,
    Error,
};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Pattern<'a> {
    meta: &'a str,
    content: &'a str,
}

impl<'a> Default for Pattern<'a> {
    fn default() -> Self {
        Self {
            meta: "//=",
            content: "//#",
        }
    }
}

impl<'a> Pattern<'a> {
    pub fn from_arg(arg: &'a str) -> Result<Self, Error> {
        let mut parts = arg.split(' ').filter(|p| !p.is_empty());
        let meta = parts.next().expect("should have at least one pattern");
        if meta.is_empty() {
            return Err("compliance pattern cannot be empty".to_string().into());
        }

        let content = parts.next().unwrap();

        Ok(Self { meta, content })
    }

    pub fn extract(
        &self,
        source: &str,
        path: &Path,
        annotations: &mut AnnotationSet,
    ) -> Result<(), Error> {
        let mut state = ParserState::Search;

        for (line_no, line) in source.lines().enumerate() {
            let content = line.trim_start();

            match core::mem::replace(&mut state, ParserState::Search) {
                ParserState::Search => {
                    let content = if content.starts_with(self.meta) {
                        &content[self.meta.len()..]
                    } else {
                        continue;
                    };

                    if content.is_empty() {
                        continue;
                    }

                    let column = line.len() - content.len();

                    let mut capture = Capture::new(line_no, column);
                    capture.meta.push(content);

                    state = ParserState::CapturingMeta(capture);
                }
                ParserState::CapturingMeta(mut capture) => {
                    if content.starts_with(self.meta) {
                        capture.meta.push(&content[self.meta.len()..]);
                        state = ParserState::CapturingMeta(capture);
                    } else if content.starts_with(self.content) {
                        capture.contents.push(&content[self.content.len()..]);
                        state = ParserState::CapturingContent(capture);
                    } else {
                        annotations.insert(capture.done(line_no, path)?);
                    }
                }
                ParserState::CapturingContent(mut capture) => {
                    if content.starts_with(self.meta) {
                        return Err("cannot set metadata while parsing content".into());
                    } else if content.starts_with(self.content) {
                        capture.contents.push(&content[self.content.len()..]);
                        state = ParserState::CapturingContent(capture);
                    } else {
                        annotations.insert(capture.done(line_no, path)?);
                    }
                }
            }
        }

        Ok(())
    }
}

enum ParserState<'a> {
    Search,
    CapturingMeta(Capture<'a>),
    CapturingContent(Capture<'a>),
}

#[derive(Debug)]
struct Capture<'a> {
    line: usize,
    column: usize,
    meta: Vec<&'a str>,
    contents: Vec<&'a str>,
}

impl<'a> Capture<'a> {
    fn new(line: usize, column: usize) -> Self {
        Self {
            line,
            column,
            meta: vec![],
            contents: vec![],
        }
    }

    fn done(&self, item_line: usize, path: &Path) -> Result<Annotation, Error> {
        let annotation = ParsedAnnotation::default();

        // TODO print meta
        eprintln!("{:#?}", &self.meta);

        // TODO concat into quote
        eprintln!("{:#?}", &self.contents);

        Ok(annotation.into())
    }
}
