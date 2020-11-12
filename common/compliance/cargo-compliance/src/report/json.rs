use super::{Reference, ReportResult, TargetReport};
use crate::{
    annotation::{AnnotationLevel, AnnotationType},
    sourcemap::Str,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    io::{BufWriter, Cursor, Error, Write},
    path::Path,
};

macro_rules! writer {
    ($writer:ident) => {
        macro_rules! w {
            ($arg: expr) => {
                write!($writer, "{}", $arg)?
            };
        }
    };
}

macro_rules! kv {
    ($comma:ident, $k:stmt, $v:stmt) => {{
        w!($comma.comma());
        $k
        w!(":");
        $v
    }};
}

macro_rules! su {
    ($v:expr) => {
        w!(format_args!(r#""{}""#, $v))
    };
}
macro_rules! s {
    ($v:expr) => {
        su!(v_jsonescape::escape($v.as_ref()))
    };
}

macro_rules! comma {
    () => {
        Comma::default()
    };
}

macro_rules! obj {
    (| $comma:ident | $s:stmt) => {{
        w!("{");
        let mut $comma = comma!();

        $s

        w!("}");
    }};
}

macro_rules! arr {
    (| $comma:ident | $s:stmt) => {{
        w!("[");
        let mut $comma = comma!();

        $s

        w!("]");
    }};
}

macro_rules! item {
    ($comma:ident, $v:stmt) => {{
        w!($comma.comma());
        $v
    }};
}

pub fn report(report: &ReportResult, file: &Path) -> Result<(), Error> {
    let mut file = BufWriter::new(File::create(file)?);

    report_writer(report, &mut file)
}

pub fn report_writer<Output: Write>(
    report: &ReportResult,
    output: &mut Output,
) -> Result<(), Error> {
    let specs = report
        .targets
        .par_iter()
        .map(|(source, report)| {
            let id = format!("{}", &source.path);
            let mut output = Cursor::new(vec![]);
            report_source(report, &mut output)?;
            let output = unsafe { String::from_utf8_unchecked(output.into_inner()) };
            Ok((id, output))
        })
        .collect::<Result<BTreeMap<String, String>, std::io::Error>>()?;

    writer!(output);

    obj!(|obj| {
        if let Some(link) = report.blob_link {
            kv!(obj, s!("blob_link"), s!(link));
        }

        kv!(
            obj,
            s!("specifications"),
            obj!(|obj| {
                for (id, spec) in &specs {
                    // don't escape `spec` since it's already been serialized to json
                    kv!(obj, s!(id), w!(spec));
                }
            })
        );

        kv!(
            obj,
            s!("annotations"),
            arr!(|arr| {
                for annotation in report.annotations {
                    item!(
                        arr,
                        obj!(|obj| {
                            kv!(obj, s!("source"), s!(annotation.source.to_string_lossy()));
                            kv!(obj, s!("target_path"), s!(annotation.target_path()));

                            if let Some(section) = annotation.target_section() {
                                kv!(obj, s!("target_section"), s!(section));
                            }

                            if annotation.anno_line > 0 {
                                kv!(obj, s!("line"), w!(annotation.anno_line));
                            }

                            if annotation.anno != AnnotationType::Citation {
                                kv!(obj, s!("type"), su!(annotation.anno));
                            }

                            if annotation.level != AnnotationLevel::Auto {
                                kv!(obj, s!("level"), su!(annotation.level));
                            }

                            if !annotation.comment.is_empty() {
                                kv!(obj, s!("comment"), s!(annotation.comment));
                            }
                        })
                    );
                }
            })
        );

        kv!(
            obj,
            s!("statuses"),
            obj!(|obj| {
                for target in report.targets.values() {
                    for (anno_id, status) in target.statuses.iter() {
                        kv!(
                            obj,
                            su!(anno_id),
                            obj!(|obj| {
                                macro_rules! status {
                                    ($field:ident) => {
                                        if status.$field > 0 {
                                            kv!(obj, su!(stringify!($field)), w!(status.$field));
                                        }
                                    };
                                }
                                status!(spec);
                                status!(incomplete);
                                status!(citation);
                                status!(test);
                                status!(exception);
                                status!(todo);

                                if !status.related.is_empty() {
                                    kv!(
                                        obj,
                                        su!("related"),
                                        arr!(|arr| {
                                            for id in &status.related {
                                                item!(arr, w!(id));
                                            }
                                        })
                                    );
                                }
                            })
                        );
                    }
                }
            })
        );

        kv!(
            obj,
            s!("refs"),
            arr!(|arr| {
                RefStatus::for_each::<_, Error>(|s| {
                    item!(
                        arr,
                        obj!(|obj| {
                            macro_rules! status {
                                ($field:ident) => {
                                    if s.$field {
                                        kv!(obj, su!(stringify!($field)), w!("true"));
                                    }
                                };
                            }

                            status!(spec);
                            status!(citation);
                            status!(test);
                            status!(exception);
                            status!(todo);

                            if s.level != AnnotationLevel::Auto {
                                kv!(obj, su!("level"), su!(s.level));
                            }
                        })
                    );

                    Ok(())
                })?
            })
        );
    });

    Ok(())
}

pub fn report_source<Output: Write>(
    report: &TargetReport,
    output: &mut Output,
) -> Result<(), Error> {
    writer!(output);

    let mut references: HashMap<usize, Vec<&Reference>> = HashMap::new();
    let mut requirements = BTreeSet::new();
    for reference in &report.references {
        if reference.annotation.anno == AnnotationType::Spec {
            requirements.insert(reference.annotation_id);
        }
        references
            .entry(reference.line)
            .or_default()
            .push(reference);
    }

    obj!(|obj| {
        if let Some(title) = &report.specification.title {
            kv!(obj, s!("title"), s!(title));
        }

        kv!(
            obj,
            s!("requirements"),
            arr!(|arr| {
                for requirement in requirements.iter() {
                    item!(arr, w!(requirement));
                }
                requirements.clear();
            })
        );

        kv!(
            obj,
            s!("sections"),
            arr!(|arr| {
                for section in report.specification.sorted_sections() {
                    item!(
                        arr,
                        obj!(|obj| {
                            kv!(obj, s!("id"), s!(section.id));
                            kv!(obj, s!("title"), s!(section.title));

                            kv!(
                                obj,
                                s!("lines"),
                                arr!(|arr| {
                                    for line in &section.lines {
                                        item!(
                                            arr,
                                            if let Some(refs) = references.get(&line.line) {
                                                report_references(
                                                    line,
                                                    refs,
                                                    &mut requirements,
                                                    output,
                                                )?;
                                            } else {
                                                // the line has no annotations so just print it
                                                s!(line);
                                            }
                                        )
                                    }
                                })
                            );

                            if !requirements.is_empty() {
                                kv!(
                                    obj,
                                    s!("requirements"),
                                    arr!(|arr| {
                                        for requirement in requirements.iter() {
                                            item!(arr, w!(requirement));
                                        }
                                        requirements.clear();
                                    })
                                );
                            }
                        })
                    );
                }
            })
        );
    });

    Ok(())
}

fn report_references<Output: Write>(
    line: &Str,
    refs: &[&Reference],
    requirements: &mut BTreeSet<usize>,
    output: &mut Output,
) -> Result<(), Error> {
    writer!(output);

    if line.is_empty() {
        s!("");
        return Ok(());
    }

    assert!(!refs.is_empty());
    arr!(|arr| {
        let mut start = line.pos;
        let end = line.pos + line.len();

        while start < end {
            let mut min_end = end;
            let current_refs = refs.iter().filter(|r| {
                if r.start <= start {
                    if start < r.end {
                        min_end = min_end.min(r.end);
                        true
                    } else {
                        false
                    }
                } else {
                    min_end = min_end.min(r.start);
                    false
                }
            });

            item!(
                arr,
                arr!(|arr| {
                    let mut status = RefStatus::default();

                    // build a list of the referenced annotations
                    item!(
                        arr,
                        arr!(|arr| {
                            for r in current_refs {
                                item!(arr, w!(r.annotation_id));
                                if r.annotation.anno == AnnotationType::Spec {
                                    requirements.insert(r.annotation_id);
                                }
                                status.on_anno(r);
                            }
                        })
                    );

                    // report on the status of this particular set of references
                    item!(arr, w!(status.id()));

                    // output the actual text
                    item!(arr, s!(line[(start - line.pos)..(min_end - line.pos)]));
                })
            );

            start = min_end;
        }
    });

    Ok(())
}

#[derive(Default)]
struct Comma(bool);

impl Comma {
    fn comma(&mut self) -> &'static str {
        if core::mem::replace(&mut self.0, true) {
            ","
        } else {
            ""
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
struct RefStatus {
    spec: bool,
    citation: bool,
    test: bool,
    exception: bool,
    todo: bool,
    level: AnnotationLevel,
}

impl RefStatus {
    fn for_each<F: FnMut(Self) -> Result<(), E>, E>(mut f: F) -> Result<(), E> {
        for level in AnnotationLevel::LEVELS.iter().copied() {
            for spec in [false, true].iter().copied() {
                for citation in [false, true].iter().copied() {
                    for test in [false, true].iter().copied() {
                        for exception in [false, true].iter().copied() {
                            for todo in [false, true].iter().copied() {
                                let status = Self {
                                    spec,
                                    citation,
                                    test,
                                    exception,
                                    todo,
                                    level,
                                };
                                f(status)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn id(self) -> usize {
        let mut id = 0;
        let mut mask = 0x1;
        let mut count = 0;

        macro_rules! field {
            ($name:ident) => {
                if self.$name {
                    id |= mask;
                }
                mask <<= 1;
                count += 1;
            };
        }

        field!(todo);
        field!(exception);
        field!(test);
        field!(citation);
        field!(spec);

        let _ = mask;

        let level = AnnotationLevel::LEVELS
            .iter()
            .copied()
            .position(|l| l == self.level)
            .unwrap();

        id += level * 2usize.pow(count);

        id
    }

    fn on_anno(&mut self, r: &Reference) {
        self.level = self.level.max(r.annotation.level);
        match r.annotation.anno {
            AnnotationType::Spec => self.spec = true,
            AnnotationType::Citation => self.citation = true,
            AnnotationType::Test => self.test = true,
            AnnotationType::Exception => self.exception = true,
            AnnotationType::Todo => self.todo = true,
        }
    }
}

impl Into<usize> for RefStatus {
    fn into(self) -> usize {
        self.id()
    }
}

#[test]
fn status_id_test() {
    let mut count = 0;
    let _ = RefStatus::for_each::<_, ()>(|s| {
        dbg!((count, s));
        assert_eq!(s.id(), count);
        count += 1;
        Ok(())
    });
}
