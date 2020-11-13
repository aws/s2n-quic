use crate::{sourcemap::Str, Error};
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
    pub title: Option<Str<'a>>,
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
    pub fn parse(self, contents: &str) -> Result<Specification, Error> {
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
    pub id: Str<'a>,
    pub title: Str<'a>,
    pub full_title: Str<'a>,
    pub lines: Vec<Str<'a>>,
}

impl<'a> Section<'a> {
    pub fn contents(&self) -> StrView {
        StrView::new(&self.lines)
    }
}

#[derive(Debug)]
pub struct StrView {
    pub value: String,
    pub byte_map: Vec<usize>,
    pub line_map: Vec<usize>,
}

impl StrView {
    pub fn new(contents: &[Str]) -> Self {
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

    pub fn ranges(&self, src: Range<usize>) -> StrRangeIter {
        StrRangeIter {
            byte_map: &self.byte_map,
            line_map: &self.line_map,
            start: src.start,
            end: src.end,
        }
    }
}

impl Deref for StrView {
    type Target = str;

    fn deref(&self) -> &str {
        &self.value
    }
}

pub struct StrRangeIter<'a> {
    byte_map: &'a [usize],
    line_map: &'a [usize],
    start: usize,
    end: usize,
}

impl<'a> Iterator for StrRangeIter<'a> {
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
