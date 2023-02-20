// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{units::*, Error, Result};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct Registry {
    definitions: BTreeMap<&'static str, Definition>,
}

impl Registry {
    pub fn define<T>(&mut self, name: &'static str, docs: &'static [&'static str], default: &T)
    where
        T: TryFromValue,
    {
        self.definitions
            .insert(name, Definition::new::<T>(docs, default.display()));
    }

    pub fn clap_args(&self) -> impl Iterator<Item = clap::Arg> + '_ {
        self.definitions.iter().map(|(name, def)| {
            clap::Arg::with_name(name)
                .long(name)
                .value_name(def.value_name)
                .help(def.help)
                .default_value(&def.default)
                .takes_value(def.takes_value)
                .long_help(&def.long_help)
        })
    }

    pub fn load_overrides(&self, matches: &clap::ArgMatches) -> Overrides {
        let mut overrides = Overrides::default();
        for (name, _) in self.definitions.iter() {
            if let Some(value) = matches.value_of(name) {
                overrides
                    .values
                    .insert(name, Override::String(value.to_string()));
            } else if matches.is_present(name) {
                overrides.values.insert(name, Override::Enabled);
            }
        }
        overrides
    }
}

#[derive(Default)]
pub struct Overrides {
    values: BTreeMap<&'static str, Override>,
    errors: BTreeMap<&'static str, Error>,
}

impl Overrides {
    pub fn resolve<T>(&mut self, name: &'static str, default: T) -> T
    where
        T: TryFromValue,
    {
        if let Some(value) = self.values.get(name) {
            match T::try_from_value(value) {
                Ok(value) => {
                    return value;
                }
                Err(err) => {
                    self.errors.insert(name, err);
                }
            }
        }

        default
    }

    pub fn errors(&self) -> impl Iterator<Item = String> + '_ {
        self.errors
            .iter()
            .map(|(name, error)| format!("{name}: {error}\n"))
    }
}

#[derive(Debug)]
struct Definition {
    help: &'static str,
    long_help: String,
    default: String,
    value_name: &'static str,
    takes_value: bool,
}

impl Definition {
    fn new<T: TryFromValue>(docs: &[&'static str], default: String) -> Self {
        Self {
            help: docs[0],
            long_help: docs.iter().map(|v| v.trim()).collect::<Vec<_>>().join("\n"),
            default,
            value_name: T::VALUE_NAME,
            takes_value: T::TAKES_VALUE,
        }
    }
}

#[derive(Debug)]
pub enum Override {
    Enabled,
    String(String),
}

pub trait TryFromValue: Sized {
    const VALUE_NAME: &'static str;
    const TAKES_VALUE: bool = true;

    fn try_from_value(value: &Override) -> Result<Self>;
    fn display(&self) -> String;
}

impl TryFromValue for bool {
    const VALUE_NAME: &'static str = "BOOL";
    const TAKES_VALUE: bool = false;

    fn try_from_value(value: &Override) -> Result<Self> {
        match value {
            Override::Enabled => Ok(true),
            Override::String(v) => match v.as_str() {
                "true" | "TRUE" | "1" | "yes" | "YES" => Ok(true),
                "false" | "FALSE" | "0" | "no" | "NO" => Ok(false),
                _ => Err(format!("invalid bool: {v:?}").into()),
            },
        }
    }

    fn display(&self) -> String {
        self.to_string()
    }
}

impl TryFromValue for u64 {
    const VALUE_NAME: &'static str = "COUNT";

    fn try_from_value(value: &Override) -> Result<Self> {
        match value {
            Override::Enabled => Err("missing value".into()),
            Override::String(v) => Ok(v.parse()?),
        }
    }

    fn display(&self) -> String {
        self.to_string()
    }
}

impl TryFromValue for Duration {
    const VALUE_NAME: &'static str = "TIME";

    fn try_from_value(value: &Override) -> Result<Self> {
        match value {
            Override::Enabled => Err("missing value".into()),
            Override::String(v) => {
                let v: humantime::Duration = v.parse()?;
                Ok(*v)
            }
        }
    }

    fn display(&self) -> String {
        if *self == Self::ZERO {
            return "0s".to_owned();
        }
        format!("{self:?}")
    }
}

impl<T: TryFromValue> TryFromValue for Option<T> {
    const VALUE_NAME: &'static str = T::VALUE_NAME;

    fn try_from_value(value: &Override) -> Result<Self> {
        if matches!(value, Override::String(v) if v == "NONE") {
            return Ok(None);
        }

        T::try_from_value(value).map(Some)
    }

    fn display(&self) -> String {
        if let Some(value) = self.as_ref() {
            value.display()
        } else {
            "NONE".to_owned()
        }
    }
}

macro_rules! try_from_value {
    ($name:ty, $value_name:literal) => {
        impl TryFromValue for $name {
            const VALUE_NAME: &'static str = $value_name;

            fn try_from_value(value: &Override) -> Result<Self> {
                match value {
                    Override::Enabled => Err("missing value".into()),
                    Override::String(v) => Ok(v.parse()?),
                }
            }

            fn display(&self) -> String {
                self.to_string()
            }
        }
    };
}

try_from_value!(Byte, "BYTES");
try_from_value!(Rate, "RATE");

impl<T: TryFromValue> TryFromValue for Vec<T> {
    const VALUE_NAME: &'static str = T::VALUE_NAME;

    fn try_from_value(value: &Override) -> Result<Self> {
        let mut out = vec![];
        if let Override::String(v) = value {
            for value in v.split(',') {
                let value = value.trim();
                let value = Override::String(value.to_owned());
                out.push(T::try_from_value(&value)?);
            }
        }
        Ok(out)
    }

    fn display(&self) -> String {
        let mut out = vec![];
        for value in self {
            out.push(value.display());
        }
        out.join(",")
    }
}
