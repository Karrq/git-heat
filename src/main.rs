extern crate git_heat;
use std::{collections::HashMap, path::PathBuf};

use clap::Parser;
use git2::Repository;
use git_heat::*;
use itertools::Itertools;
use parking_lot::RwLock;
use time::OffsetDateTime;

use rayon::prelude::*;

#[derive(Parser)]
struct Args {
    #[clap(long, default_value = "")]
    /// Lower bound of datetime range
    ///
    /// Default: UNIX_EPOCH
    from: String,

    #[clap(long, default_value = "")]
    /// Upper bound of datetime range
    ///
    /// Default: Now
    to: String,

    #[clap(parse(from_os_str), default_value = ".")]
    /// Path to repo to inspect
    repo: PathBuf,

    #[clap(long)]
    /// Format output as JSON
    json: bool,
}

fn main() {
    let args = Args::parse();

    let repo = Repository::open(args.repo).expect("not a valid git repo");

    let format = time::format_description::parse("[year]-[month]-[day]").unwrap();

    let from = OffsetDateTime::parse(&args.from, &format).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let to = OffsetDateTime::parse(&args.to, &format).unwrap_or_else(|_| OffsetDateTime::now_utc());

    let commits =
        commits_in_date_range(from, to, &repo).expect("unable to retrieve commits in date range");
    let pairs = pair_commits(commits);
    let diffs = pairs
        .map(|(new, old)| {
            get_diff_of_commits(old, new, &repo).expect("unable to get diff from commits")
        })
        //collect diffs so we can parallelize the reduce later
        .collect::<Vec<_>>();

    let renames = RwLock::new(HashMap::new());
    let changes = diffs
        .into_par_iter()
        .map(|diff| get_files_changed(diff, &renames));

    let mut changes = changes
        .reduce_with(|mut acc, changes| {
            for (k, v) in changes.into_iter() {
                *acc.entry(k).or_default() += v;
            }
            acc
        })
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();

    changes.sort_by_key(|(_, v)| -(*v as i64));

    if !args.json {
        for (file, changes) in changes {
            println!("{file:?}: {changes}")
        }
    } else {
        let v = serde_json::to_value(changes).unwrap();
        println!("{}", serde_json::to_string_pretty(&v).unwrap())
    }
}
