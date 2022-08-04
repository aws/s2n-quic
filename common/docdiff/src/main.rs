// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{anyhow, Context, Error, Result};
use duct::cmd;
use glob::glob;
use once_cell::sync::OnceCell;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use scraper::{ElementRef, Html, Selector};
use selectors::attr::CaseSensitivity;
use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Arguments {
    #[structopt(short, long)]
    output: Option<PathBuf>,

    #[structopt(name = "CRATE")]
    crate_name: String,
}

macro_rules! sel {
    ($sel:expr) => {{
        static SEL: OnceCell<Selector> = OnceCell::new();
        SEL.get_or_init(|| Selector::parse($sel).unwrap())
    }};
}

fn main() -> Result<()> {
    let Arguments { output, crate_name } = Arguments::from_args();

    let dump = Dump::new(&crate_name)?;

    let path = cmd!("cargo", "pkgid", &crate_name).stdout_capture().run()?;
    let path = String::from_utf8(path.stdout)?;
    let path = path
        .trim_start_matches("file://")
        .split('#')
        .next()
        .unwrap();
    let project = PathBuf::from_str(path)?;

    let output = if let Some(output) = output {
        output
    } else {
        project.clone()
    };

    let mut settings = insta::Settings::new();
    settings.set_input_file(&project.join("src").join("lib.rs"));
    settings.set_snapshot_path(output);
    settings.set_prepend_module_to_snapshot(false);

    settings.bind(|| {
        insta::assert_display_snapshot!("api", dump);
    });

    Ok(())
}

struct Dump {
    entries: Vec<Entry>,
}

impl fmt::Display for Dump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for entry in &self.entries {
            write!(f, "{}", entry)?;
        }
        Ok(())
    }
}

impl Dump {
    fn new(crate_name: &str) -> Result<Self> {
        cmd!(
            "cargo",
            "doc",
            "--all-features",
            "--workspace",
            "--target-dir",
            "target/docdiff"
        )
        .env("RUSTFLAGS", "--cfg docdiff")
        .env("RUSTDOCFLAGS", "--cfg docdiff")
        .stdout_path("/dev/null")
        .run()?;

        let paths = glob(&format!(
            "target/docdiff/doc/{}/**/*.html",
            crate_name.replace('-', "_")
        ))?
        .collect::<Vec<_>>();

        let paths: Vec<_> = paths
            .into_par_iter()
            .map(|path| {
                let path = path?;
                index_file(&path).with_context(|| format!("failed to parse: {}", path.display()))
            })
            .collect();

        let mut entries = vec![];
        let mut has_error = false;
        for result in paths {
            match result {
                Ok(e) => entries.extend(e),
                Err(err) => {
                    has_error = true;
                    eprintln!("error {:?}", err);
                }
            }
        }

        if has_error {
            return Err(anyhow!("bailing due to errors"));
        }

        // make sure things are sorted for nicer diffs
        entries.sort();
        entries.dedup();

        Ok(Self { entries })
    }
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
struct Entry {
    context: Arc<Fqn>,
    signature: String,
    kind: Kind,
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let action = match (self.context.kind, self.kind) {
            (Kind::Struct, Kind::Trait) | (Kind::Enum, Kind::Trait) => "implements",
            (Kind::Struct, Kind::NonExhaustive)
            | (Kind::Enum, Kind::NonExhaustive)
            | (Kind::Variant, Kind::NonExhaustive) => "is",
            (Kind::Trait, Kind::Type) | (Kind::Trait, Kind::Constant) => "associates",
            _ => "exports",
        };

        write!(
            f,
            "{} {} {} {}",
            self.context.kind, self.context.path, action, self.kind
        )?;

        if !self.signature.is_empty() {
            writeln!(f, ":")?;
        } else {
            writeln!(f)?;
        }

        for line in self.signature.split_terminator('\n') {
            writeln!(f, "  {}", line.trim_end())?;
        }

        writeln!(f)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
struct Fqn {
    path: String,
    kind: Kind,
}

impl FromStr for Fqn {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(sig) = s.strip_prefix("Type Definition ") {
            return Ok(Self {
                kind: Kind::Typedef,
                path: sig.to_string(),
            });
        }

        let (kind, path) = s
            .trim()
            .split_once(' ')
            .ok_or_else(|| anyhow!("invalid fqn {}", s))?;
        let kind = kind.parse()?;
        let path = path.into();
        Ok(Self { path, kind })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord)]
enum Kind {
    Crate,
    Module,
    Struct,
    Enum,
    NonExhaustive,
    Variant,
    Field,
    Function,
    Type,
    Typedef,
    Static,
    Constant,
    Trait,
    List,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Kind::Struct => "struct",
            Kind::Enum => "enum",
            Kind::Trait => "trait",
            Kind::Function => "function",
            Kind::Module => "module",
            Kind::Crate => "crate",
            Kind::Type => "type",
            Kind::Typedef => "typedef",
            Kind::Static => "static",
            Kind::Constant => "constant",
            Kind::List => "list",
            Kind::Variant => "variant",
            Kind::Field => "field",
            Kind::NonExhaustive => "non-exhaustive",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for Kind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "Struct" | "struct" => Self::Struct,
            "Enum" | "enum" => Self::Enum,
            "Trait" | "trait" => Self::Trait,
            "Function" | "function" | "fn" => Self::Function,
            "Module" | "mod" => Self::Module,
            "Crate" | "crate" => Self::Crate,
            "Type" | "type" => Self::Type,
            "Static" | "static" => Self::Static,
            "Constant" | "constant" => Self::Constant,
            "List" => Self::List,
            _ => return Err(anyhow!("Unsupported kind {}", s)),
        })
    }
}

fn index_file(path: &Path) -> Result<Vec<Entry>> {
    let mut results = vec![];

    let contents = std::fs::read_to_string(path)?;
    let document = Html::parse_document(&contents);

    let context: Fqn = if let Some(fqn) = document.select(sel!(".fqn")).next() {
        el_to_string(fqn).parse()?
    } else {
        eprintln!("skipping {}", path.display());
        return Ok(results);
    };
    let context = Arc::new(context);

    for el in document
        .select(sel!("#main .import-item a[title]"))
        .chain(document.select(sel!("#main .module-item a[title]")))
    {
        let el = ElementRef::wrap(*el).unwrap();
        let title = el.value().attr("title").unwrap();
        if let Ok(entry) = parse_entry_title(title, &context) {
            results.push(entry);
        }
    }

    let items = [
        (sel!(".impl-items .method:not(.trait-impl)"), Kind::Function),
        (sel!(".impl-items .associatedconstant"), Kind::Constant),
        (sel!(".methods .method[id^=tymethod]"), Kind::Function),
        (sel!(".typedef"), Kind::Typedef),
        (sel!("#synthetic-implementations-list .impl"), Kind::Trait),
        (sel!(".structfield"), Kind::Field),
    ];

    for (sel, kind) in items.iter().copied() {
        for el in document.select(sel) {
            let signature = el_to_string(el);
            results.push(Entry {
                kind,
                signature,
                context: context.clone(),
            })
        }
    }

    let trait_impl_details = document.select(sel!("#trait-implementations-list details"));
    for trait_impl in document.select(sel!("#trait-implementations-list details .impl")) {
        let signature = el_to_string(trait_impl);
        results.push(Entry {
            kind: Kind::Trait,
            signature: signature.clone(),
            context: context.clone(),
        });

        for trait_impl in trait_impl_details {
            let context = Arc::new(Fqn {
                kind: Kind::Trait,
                path: signature.clone(),
            });
            let constants = trait_impl
                .select(sel!(
                    ".impl-items .trait-impl.associatedconstant:not(.hidden)"
                ))
                .map(|t| (t, Kind::Constant));
            let types = trait_impl
                .select(sel!(".impl-items .trait-impl.type:not(.hidden)"))
                .map(|t| (t, Kind::Type));
            let items = types.chain(constants);
            for (item, kind) in items {
                let signature = el_to_string(item);
                results.push(Entry {
                    kind,
                    signature,
                    context: context.clone(),
                })
            }
        }
    }

    // search for variants
    for variant in document.select(sel!("#main > .variant")) {
        let name = el_to_string(variant)
            .trim_start_matches("Fields of ")
            .to_string();

        results.push(Entry {
            kind: Kind::Variant,
            signature: name.clone(),
            context: context.clone(),
        });

        // look for the non-exhaustive marker
        for sibling in variant.next_siblings().flat_map(ElementRef::wrap) {
            // we're on to the next section
            if sibling
                .value()
                .has_class(".variant", CaseSensitivity::CaseSensitive)
            {
                break;
            }

            if sibling
                .value()
                .has_class("non-exhaustive", CaseSensitivity::CaseSensitive)
            {
                let path = format!("{}::{}", context.path, name);
                let context = Arc::new(Fqn {
                    kind: Kind::Variant,
                    path,
                });
                results.push(Entry {
                    kind: Kind::NonExhaustive,
                    signature: String::new(),
                    context,
                });
            }
        }
    }

    // search for variant fields
    for variant in document.select(sel!(".sub-variant[id^=variant\\.]")) {
        let name = variant
            .value()
            .id()
            .unwrap()
            .trim_start_matches("variant.")
            .trim_end_matches(".fields");

        let path = format!("{}::{}", context.path, name);
        let context = Arc::new(Fqn {
            kind: Kind::Variant,
            path,
        });

        for field in variant.select(sel!(".variant")) {
            let signature = el_to_string(field);
            results.push(Entry {
                kind: Kind::Field,
                signature,
                context: context.clone(),
            })
        }
    }

    for el in document
        .select(sel!("#variants"))
        .chain(document.select(sel!("#fields")))
    {
        if el_to_string(el).contains("(Non-exhaustive)") {
            results.push(Entry {
                kind: Kind::NonExhaustive,
                signature: String::new(),
                context: context.clone(),
            })
        }
    }

    Ok(results)
}

fn parse_entry_title(title: &str, context: &Arc<Fqn>) -> Result<Entry> {
    let (kind, sig) = title
        .trim()
        .split_once(' ')
        .ok_or_else(|| anyhow!("invalid entry title: {}", title))?;

    if let Ok(kind) = kind.parse() {
        let signature = sig.to_string();
        let context = context.clone();
        Ok(Entry {
            context,
            signature,
            kind,
        })
    } else {
        let kind_p = sig.parse()?;
        let signature = kind.to_string();
        let context = context.clone();
        Ok(Entry {
            context,
            signature,
            kind: kind_p,
        })
    }
}

fn el_to_string(el: ElementRef) -> String {
    fn traverse(el: ElementRef, out: &mut String) {
        for child in el.children() {
            if let Some(text) = child.value().as_text() {
                let mut has_ua0 = false;
                out.extend(text.text.chars().filter_map(|c| {
                    if c == '\u{a0}' {
                        if core::mem::replace(&mut has_ua0, true) {
                            None
                        } else {
                            Some('\n')
                        }
                    } else {
                        has_ua0 = false;
                        Some(c)
                    }
                }));
            } else if let Some(el) = child.value().as_element() {
                if el.name() == "button" {
                    continue;
                }

                let case = CaseSensitivity::CaseSensitive;

                if el.has_class("out-of-band", case)
                    || el.has_class("srclink", case)
                    || el.has_class("collapse-toggle", case)
                    || el.has_class("since", case)
                    || el.has_class("docblock", case)
                {
                    continue;
                }

                if let Some(Fqn { path, .. }) = el.attr("title").and_then(|v| v.parse().ok()) {
                    out.push_str(&path);
                    continue;
                }

                let child = ElementRef::wrap(child).unwrap();
                traverse(child, out);

                // put a newline after attributes
                if el.has_class("code-attribute", case) {
                    out.push('\n');
                }
            }
        }
    }

    let mut out = String::new();
    traverse(el, &mut out);
    out
}
