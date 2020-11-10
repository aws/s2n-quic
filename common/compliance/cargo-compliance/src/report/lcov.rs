use super::TargetReport;
use crate::annotation::AnnotationType;
use std::{
    collections::HashSet,
    io::{Error, Write},
};

const IMPL_BLOCK: &str = "0,0";
const TEST_BLOCK: &str = "1,0";

macro_rules! line {
    ($value:expr) => {
        $value.line
    };
}

macro_rules! record {
    ($block:expr, $line_hits:ident, $line:expr, $title:expr, $count:expr) => {
        if $count != 0 {
            $line_hits.insert($line);
        }
        put!("BRDA:{},{},{}", $line, $block, $count);
        if let Some(title) = $title {
            let mut title_count = $count;
            if title_count != 0 {
                if !$line_hits.contains(&line!(title)) {
                    // mark the title as recorded
                    $line_hits.insert(line!(title));
                } else {
                    // the title was already recorded
                    title_count = 0;
                }
            }

            put!("FNDA:{},{}", title_count, title);
            put!("BRDA:{},{},{}", line!(title), $block, title_count);
        }
    };
}

#[allow(clippy::cognitive_complexity)]
pub fn report<Output: Write>(report: &TargetReport, output: &mut Output) -> Result<(), Error> {
    macro_rules! put {
        ($($arg:expr),* $(,)?) => {
            writeln!(output $(, $arg)*)?;
        };
    }

    put!("TN:Compliance");
    let relative =
        pathdiff::diff_paths(report.target.path.local(), std::env::current_dir()?).unwrap();
    put!("SF:{}", relative.display());

    // record all sections
    for section in report.specification.sections.values() {
        let title = section.full_title;
        put!("FN:{},{}", line!(title), title);
    }

    put!("FNF:{}", report.specification.sections.len());

    // TODO replace with interval set
    let mut cited_lines = HashSet::new();
    let mut tested_lines = HashSet::new();

    // record all references to specific sections
    for reference in &report.references {
        let title = if let Some(section_id) = reference.annotation.target_section() {
            let section = report.specification.sections.get(section_id).unwrap();
            Some(section.full_title)
        } else {
            None
        };

        let line = line!(reference);

        macro_rules! citation {
            ($count:expr) => {
                record!(IMPL_BLOCK, cited_lines, line, title, $count);
            };
        }

        macro_rules! test {
            ($count:expr) => {
                record!(TEST_BLOCK, tested_lines, line, title, $count);
            };
        }

        match reference.annotation.anno {
            AnnotationType::Test => {
                citation!(0);
                test!(1);
            }
            AnnotationType::Citation => {
                citation!(1);
                test!(0);
            }
            AnnotationType::Exception => {
                // mark exceptions as fully covered
                citation!(1);
                test!(1);
            }
            AnnotationType::Spec | AnnotationType::Todo => {
                // specifications highlight the line as significant, but no coverage
                citation!(0);
                test!(0);
            }
        }
    }

    // mark any lines that were both cited and tested as covered
    for line in cited_lines.intersection(&tested_lines) {
        put!("DA:{},{}", line, 1);
    }

    // mark any lines that didn't appear in both as uncovered
    for line in cited_lines.symmetric_difference(&tested_lines) {
        put!("DA:{},{}", line, 0);
    }

    put!("end_of_record");

    Ok(())
}
