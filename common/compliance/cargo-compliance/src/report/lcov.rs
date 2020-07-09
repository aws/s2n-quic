use super::SourceReport;
use crate::annotation::AnnotationType;
use std::io::{Error, Write};

const IMPL_BLOCK: &str = "0,0";
const TEST_BLOCK: &str = "1,0";

macro_rules! line {
    ($value:expr) => {
        // lines are 1 index
        $value.line + 1
    };
}

pub fn report<Output: Write>(report: &SourceReport, output: &mut Output) -> Result<(), Error> {
    macro_rules! put {
        ($($arg:expr),* $(,)?) => {
            writeln!(output $(, $arg)*)?;
        };
    }

    put!("TN:Compliance");
    let relative =
        pathdiff::diff_paths(report.source.path.local(), std::env::current_dir()?).unwrap();
    put!("SF:{}", relative.display());

    // record all sections
    for section in report.specification.sections.values() {
        let title = section.full_title;
        put!("FN:{},{}", line!(title), title);
    }

    put!("FNF:{}", report.specification.sections.len());

    // set all significant lines to 0
    for section in report.specification.sections.values() {
        let title_line = line!(section.full_title);
        put!("DA:{},0", title_line);
        put!("BRDA:{},{},0", title_line, TEST_BLOCK);
        put!("BRDA:{},{},0", title_line, IMPL_BLOCK);
        for line in &section.lines {
            if !line.is_empty() {
                let line = line!(line);
                put!("DA:{},0", line);
                put!("BRDA:{},{},0", line, TEST_BLOCK);
                put!("BRDA:{},{},0", line, IMPL_BLOCK);
            }
        }
    }

    // record all references to specific sections
    for reference in &report.references {
        let title = if let Some(section_id) = reference.annotation.section() {
            let section = report.specification.sections.get(section_id).unwrap();
            Some(section.full_title)
        } else {
            None
        };

        let line = line!(reference);

        macro_rules! citation {
            () => {
                put!("BRDA:{},{},1", line, IMPL_BLOCK);
                if let Some(title) = title {
                    put!("FNDA:1,{}", title);
                    put!("BRDA:{},{},1", line!(title), IMPL_BLOCK);
                }
            };
        }

        macro_rules! test {
            () => {
                put!("DA:{},1", line);
                put!("BRDA:{},{},1", line, TEST_BLOCK);
                if let Some(title) = title {
                    put!("FNDA:1,{}", title);
                    put!("DA:{},1", line!(title));
                    put!("BRDA:{},{},1", line!(title), TEST_BLOCK);
                }
            };
        }

        match reference.annotation.anno {
            AnnotationType::Test => {
                test!();
            }
            AnnotationType::Citation => {
                citation!();
            }
            AnnotationType::Exception => {
                // mark exceptions as covered
                citation!();
                test!();
            }
            AnnotationType::Spec => {
                // it's just a reference, skip it
                continue;
            }
        }
    }

    put!("end_of_record");

    Ok(())
}
