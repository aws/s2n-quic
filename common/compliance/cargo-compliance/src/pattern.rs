use crate::{
    annotation::{Annotation, AnnotationSet},
    parser::ParsedAnnotation,
    Error,
};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Pattern<'a> {
    open: &'a str,
    close: Option<&'a str>,
}

impl<'a> Default for Pattern<'a> {
    fn default() -> Self {
        Self {
            open: "//#",
            close: None,
        }
    }
}

impl<'a> Pattern<'a> {
    pub fn from_arg(arg: &'a str) -> Result<Self, Error> {
        let mut parts = arg.split(' ').filter(|p| !p.is_empty());
        let open = parts.next().expect("should have at least one pattern");
        if open.is_empty() {
            return Err("compliance pattern cannot be empty".to_string().into());
        }

        let close = parts.next();

        Ok(Self { open, close })
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
                    let open = if content.starts_with(self.open) {
                        &content[self.open.len()..]
                    } else {
                        continue;
                    };

                    if open.is_empty() {
                        continue;
                    }

                    let column = line.len() - open.len();

                    if let Some(close) = self.close {
                        if let Some(close_offset) = open.find(close) {
                            let content = &open[..close_offset];
                            let mut capture = Capture::new(line_no, column);
                            capture.push(content);
                            annotations.insert(capture.done(line_no, path)?);
                            continue;
                        }
                    }

                    // the first line needs to start with metadata
                    if !open.starts_with('!') {
                        continue;
                    }

                    let mut capture = Capture::new(line_no, column);
                    capture.push(open);

                    state = ParserState::Capturing(capture);
                }
                ParserState::Capturing(mut capture) => {
                    if let Some(close) = self.close {
                        if let Some(close_offset) = content.find(close) {
                            let content = &content[..close_offset];
                            capture.push(content);
                            annotations.insert(capture.done(line_no, path)?);
                            continue;
                        } else {
                            capture.push(content);
                        }
                    } else if content.starts_with(self.open) {
                        capture.push(&content[self.open.len()..]);
                    } else {
                        annotations.insert(capture.done(line_no, path)?);
                        continue;
                    }

                    state = ParserState::Capturing(capture);
                }
            }
        }

        Ok(())
    }
}

enum ParserState<'a> {
    Search,
    Capturing(Capture<'a>),
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

    fn push(&mut self, content: &'a str) {
        let content = content.trim();
        if content.starts_with('!') && self.contents.is_empty() {
            self.meta.push(&content[1..].trim_start());
        } else {
            self.contents.push(content);
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
