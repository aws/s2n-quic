// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Section, Specification, Str};
use crate::{sourcemap::LinesIter, Error};
use core::ops::Deref;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref SECTION_HEADER_RE: Regex = Regex::new(r"^(([A-Z]\.)?[0-9\.]+)\s+(.*)").unwrap();
    static ref APPENDIX_HEADER_RE: Regex = Regex::new(r"^Appendix ([A-Z]\.)\s+(.*)").unwrap();

    /// Table of contents have at least 5 periods
    static ref TOC_RE: Regex = Regex::new(r"\.{5,}").unwrap();
}

pub fn parse(contents: &str) -> Result<Specification, Error> {
    let mut parser = Parser::default();

    for line in LinesIter::new(contents) {
        parser.on_line(line)?;
    }

    let spec = parser.done()?;

    if cfg!(debug_assertions) {
        for section in spec.sections.values() {
            for content in &section.lines {
                assert_eq!(
                    content.value,
                    &contents[content.range()],
                    "ranges are incorrect expected {:?}, actual {:?}",
                    {
                        let start =
                            (content.value.as_ptr() as usize) - (contents.as_ptr() as usize);
                        start..(start + content.value.len())
                    },
                    content.range(),
                );
            }
        }
    }

    Ok(spec)
}

#[derive(Debug, Default)]
pub struct Parser<'a> {
    spec: Specification<'a>,
    state: ParserState<'a>,
}

#[derive(Debug)]
pub enum ParserState<'a> {
    Init,
    Section {
        id: Str<'a>,
        section: Section<'a>,
        indent: usize,
    },
}

impl<'a> Default for ParserState<'a> {
    fn default() -> Self {
        Self::Init
    }
}

fn section_header(line: Str) -> Option<(Str, Section)> {
    if let Some(info) = SECTION_HEADER_RE.captures(&line) {
        let id = info.get(1)?;
        let title = info.get(3)?;

        if TOC_RE.is_match(title.as_str()) {
            return None;
        }

        let id = line.slice(id.range()).trim_end_matches('.');
        let title = line.slice(title.range());

        Some((
            id,
            Section {
                id,
                title,
                full_title: line.trim(),
                lines: vec![],
            },
        ))
    } else if let Some(info) = APPENDIX_HEADER_RE.captures(&line) {
        let id = info.get(1)?;
        let title = info.get(2)?;

        if TOC_RE.is_match(title.as_str()) {
            return None;
        }

        let id = line.slice(id.range()).trim_end_matches('.');
        let title = line.slice(title.range());

        Some((
            id,
            Section {
                id,
                title,
                full_title: line.trim(),
                lines: vec![],
            },
        ))
    } else {
        None
    }
}

impl<'a> Parser<'a> {
    pub fn on_line(&mut self, line: Str<'a>) -> Result<(), Error> {
        // remove footer marker
        if line.deref() == "\u{c}" {
            return Ok(());
        }

        match core::mem::replace(&mut self.state, ParserState::Init) {
            ParserState::Init => {
                if let Some((id, section)) = section_header(line) {
                    self.state = ParserState::Section {
                        id,
                        section,
                        indent: core::usize::MAX,
                    };
                }
            }
            ParserState::Section {
                id,
                mut section,
                indent,
            } => {
                let line_indent = line.indentation();

                // dedup whitespace
                if line_indent == line.len()
                    && section.lines.last().map(|l| !l.is_empty()).unwrap_or(false)
                {
                    section.lines.push(line.trim());

                    // most likely the footer/header
                    self.state = ParserState::Section {
                        id,
                        section,
                        indent,
                    };

                    return Ok(());
                }

                if line_indent == 0 {
                    if let Some((new_id, new_section)) = section_header(line) {
                        self.on_section(id, section, indent);
                        self.state = ParserState::Section {
                            id: new_id,
                            section: new_section,
                            indent: core::usize::MAX,
                        };
                    } else {
                        // most likely the footer/header
                        self.state = ParserState::Section {
                            id,
                            section,
                            indent,
                        };
                    }

                    return Ok(());
                }

                section.lines.push(line);

                self.state = ParserState::Section {
                    id,
                    section,
                    indent: indent.min(line_indent),
                };
            }
        }

        Ok(())
    }

    fn on_section(&mut self, id: Str<'a>, mut section: Section<'a>, indent: usize) {
        for content in &mut section.lines {
            if !content.is_empty() {
                let range = indent..content.len();
                *content = content.slice(range);
            }
        }

        // remove last whitespace
        if section.lines.last().map(|l| l.is_empty()).unwrap_or(false) {
            section.lines.pop();
        }

        self.spec.sections.insert(id.value, section);
    }

    pub fn done(mut self) -> Result<Specification<'a>, Error> {
        match core::mem::replace(&mut self.state, ParserState::Init) {
            ParserState::Init => Ok(self.spec),
            ParserState::Section {
                id,
                section,
                indent,
            } => {
                self.on_section(id, section, indent);
                Ok(self.spec)
            }
        }
    }
}

macro_rules! ietf_test {
    ($name:ident, $file:expr) => {
        #[ignore] // TODO: https://github.com/awslabs/s2n-quic/issues/649
        #[test]
        fn $name() {
            let res = parse(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../specs/",
                $file
            )))
            .unwrap();
            insta::assert_debug_snapshot!($file, res);
        }
    };
}

ietf_test!(quic_transport_29, "draft-ietf-quic-transport-29.txt");
ietf_test!(rfc2616, "rfc2616.txt");
ietf_test!(rfc791, "rfc791.txt");
