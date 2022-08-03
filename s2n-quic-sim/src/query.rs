// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stats::{self, Connection, Parameters, Stats},
    Result,
};
use anyhow::anyhow;
use std::{collections::HashMap, fs, io};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Query {
    #[structopt(long)]
    header: Option<Option<bool>>,

    #[structopt(long)]
    clients: Option<Option<bool>>,

    #[structopt(long)]
    servers: Option<Option<bool>>,

    #[structopt(long, short)]
    filter: Vec<stats::Filter>,

    #[structopt(long, short, possible_values = &*stats::QUERY_NAMES)]
    query: Vec<stats::Query>,

    #[structopt(long)]
    with_seed: bool,

    input: Option<String>,
}

impl Query {
    pub fn run(&self) -> Result {
        let input: Box<dyn io::Read> = if self.input.as_ref().map_or(true, |v| v == "-") {
            let reader = io::stdin();
            Box::new(reader)
        } else {
            let reader = fs::File::open(self.input.as_ref().unwrap())?;
            let reader = io::BufReader::new(reader);
            Box::new(reader)
        };

        if self.query.is_empty() {
            return Err(anyhow!("must specify at least one query"));
        }

        let with_seed = self.with_seed;

        let queries = &self.query;
        let filters = &self.filter;

        let header = self.header.map_or(true, |v| v.unwrap_or(true));
        if header {
            let seed = if with_seed {
                Some(("seed", "id"))
            } else {
                None
            };
            let queries = queries.iter().map(|q| q.name);
            if emit(seed, queries).is_err() {
                return Ok(());
            }
        }

        let mut values = vec![];

        let mut q = |p: &Parameters, conn: &Connection, connections: &[Connection]| -> Result {
            for filter in filters {
                if !filter.apply(p, conn, connections) {
                    return Ok(());
                }
            }

            values.clear();

            for query in queries {
                if let Some(value) = query.apply(p, conn, connections) {
                    values.push(value);
                } else {
                    return Ok(());
                }
            }

            let seed = if with_seed {
                Some((conn.seed, conn.id()))
            } else {
                None
            };

            emit(seed, values.iter())?;

            Ok(())
        };

        let clients = self.clients.map_or(true, |v| v.unwrap_or(true));
        let servers = self.servers.map_or(true, |v| v.unwrap_or(true));

        let reader = crate::stats::Stats::reader(input);

        let mut acc: HashMap<u64, Vec<Connection>> = HashMap::new();

        for stat in reader {
            match stat? {
                Stats::Setup(_) => {
                    // unused
                }
                Stats::Parameters(p) => {
                    if let Some(connections) = acc.remove(&p.seed) {
                        for conn in &connections {
                            if q(&p, conn, &connections).is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
                Stats::Connection(c) => {
                    if (clients && c.client_id.is_some()) || (servers && c.server_id.is_some()) {
                        acc.entry(c.seed).or_default().push(c);
                    }
                }
            }
        }

        Ok(())
    }
}

fn emit<
    Seed: core::fmt::Display,
    Id: core::fmt::Display,
    I: Iterator<Item = V>,
    V: core::fmt::Display,
>(
    seed: Option<(Seed, Id)>,
    i: I,
) -> io::Result<()> {
    use io::Write;
    let stdout = io::stdout();
    let mut o = stdout.lock();

    let mut has_written = false;

    if let Some((seed, id)) = seed {
        write!(o, "{}\t{}", seed, id)?;
        has_written = true;
    }

    for value in i {
        if has_written {
            write!(o, "\t{}", value)?;
        } else {
            write!(o, "{}", value)?;
        }
        has_written = true;
    }
    writeln!(o)?;
    Ok(())
}
