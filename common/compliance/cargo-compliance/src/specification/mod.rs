use crate::Error;
use core::{
    cmp::Ordering,
    fmt,
    ops::{Deref, Range},
    str::FromStr,
};
use std::collections::HashMap;

pub mod ietf;

#[derive(Default)]
pub struct Specification<'a> {
    pub title: Option<Content<'a>>,
    pub sections: HashMap<&'a str, Section<'a>>,
}

impl<'a> fmt::Debug for Specification<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Specification")
            .field("title", &self.title)
            .field("sections", &self.sorted_sections())
            .finish()
    }
}

impl<'a> Specification<'a> {
    pub fn sorted_sections(&self) -> Vec<&Section<'a>> {
        let mut sections: Vec<_> = self.sections.values().collect();

        sections.sort_by(|a, b| match a.title.line.cmp(&b.title.line) {
            Ordering::Equal => a.cmp(b),
            ordering => ordering,
        });

        sections
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub enum Format {
    Auto,
    IETF,
}

impl Default for Format {
    fn default() -> Self {
        Self::Auto
    }
}

impl Format {
    pub fn parse<'a>(self, contents: &'a str) -> Result<Specification<'a>, Error> {
        match self {
            Self::Auto => ietf::parse(contents),
            Self::IETF => ietf::parse(contents),
        }
    }
}

impl FromStr for Format {
    type Err = Error;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        match v {
            "AUTO" => Ok(Self::Auto),
            "IETF" => Ok(Self::IETF),
            _ => Err(format!("Invalid spec type {:?}", v).into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Section<'a> {
    pub id: Content<'a>,
    pub title: Content<'a>,
    pub full_title: Content<'a>,
    pub lines: Vec<Content<'a>>,
}

impl<'a> Section<'a> {
    pub fn contents(&self) -> ContentView {
        ContentView::new(&self.lines)
    }
}

pub struct ContentView {
    pub value: String,
    pub byte_map: Vec<usize>,
    pub line_map: Vec<usize>,
}

impl ContentView {
    pub fn new(contents: &[Content]) -> Self {
        let mut value = String::new();
        let mut byte_map = vec![];
        let mut line_map = vec![];

        for chunk in contents {
            let chunk = chunk.trim();
            if !chunk.is_empty() {
                value.push_str(chunk.deref());
                value.push(' ');
                let mut range = chunk.range();
                range.end += 1; // account for new line
                line_map.extend(range.clone().map(|_| chunk.line));
                byte_map.extend(range);
            }
        }

        debug_assert_eq!(value.len(), byte_map.len());
        debug_assert_eq!(value.len(), line_map.len());

        Self {
            value,
            byte_map,
            line_map,
        }
    }

    pub fn ranges(&self, src: Range<usize>) -> ContentRangeIter {
        ContentRangeIter {
            byte_map: &self.byte_map,
            line_map: &self.line_map,
            start: src.start,
            end: src.end,
        }
    }
}

impl Deref for ContentView {
    type Target = str;

    fn deref(&self) -> &str {
        &self.value
    }
}

pub struct ContentRangeIter<'a> {
    byte_map: &'a [usize],
    line_map: &'a [usize],
    start: usize,
    end: usize,
}

impl<'a> Iterator for ContentRangeIter<'a> {
    type Item = (usize, Range<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.start == self.end {
            return None;
        }

        let start_target = self.byte_map[self.start];
        let line = self.line_map[self.start];
        let mut range = start_target..start_target;
        self.start += 1;

        for i in self.start..self.end {
            let target = self.byte_map[i];
            if range.end == target - 1 {
                range.end = target;
                debug_assert_eq!(line, self.line_map[i], "chunks should only span a line");
                self.start += 1;
            } else {
                break;
            }
        }

        Some((line, range))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Content<'a> {
    pub value: &'a str,
    pub pos: usize,
    pub line: usize,
}

impl<'a> fmt::Debug for Content<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<'a> fmt::Display for Content<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<'a> Content<'a> {
    pub fn indentation(&self) -> usize {
        let trimmed_line = self.trim_start();
        self.len() - trimmed_line.len()
    }

    pub fn slice(&self, bounds: Range<usize>) -> Self {
        let pos = self.pos + bounds.start;
        let value = &self.value[bounds];
        Self {
            value,
            pos,
            line: self.line,
        }
    }

    pub fn range(&self) -> Range<usize> {
        let pos = self.pos;
        pos..(pos + self.value.len())
    }

    pub fn trim(&self) -> Self {
        let value = self.value.trim_start();
        let pos = self.pos + (self.len() - value.len());
        let value = value.trim_end();
        Self {
            value,
            pos,
            line: self.line,
        }
    }

    pub fn trim_end_matches(&self, pat: char) -> Self {
        let value = self.value.trim_end_matches(pat);
        Self {
            value,
            pos: self.pos,
            line: self.line,
        }
    }
}

impl<'a> Deref for Content<'a> {
    type Target = str;

    fn deref(&self) -> &str {
        self.value
    }
}
