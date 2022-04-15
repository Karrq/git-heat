extern crate git_heat;
use std::{collections::HashMap, path::PathBuf};

use clap::Parser;
use git2::Repository;
use git_heat::*;
use parking_lot::RwLock;
use time::OffsetDateTime;

use rayon::prelude::*;

#[derive(Parser)]
struct Args {
    /// Date range
    ///
    /// Specify in natural english
    #[clap(last = true)]
    date: Vec<String>,

    #[clap(long, parse(from_os_str), default_value = ".")]
    /// Path to repo to inspect
    repo: PathBuf,

    /// Drop all changes < than min
    #[clap(long, default_value = "0")]
    min: u32,

    #[clap(long)]
    /// Format output as JSON
    json: bool,
}

fn main() {
    let args = Args::parse();

    let repo = Repository::open(args.repo).expect("not a valid git repo");

    let (from, to) = {
        let default = (OffsetDateTime::UNIX_EPOCH, OffsetDateTime::now_utc());
        let datetime = args.date.join(" ");
        let datetime = two_timer::parse(&datetime, None).ok();

        match datetime {
            None => default,
            Some((from, to, _)) => {
                let from_ts = from.timestamp();
                let to_ts = to.timestamp();
                match (
                    OffsetDateTime::from_unix_timestamp(from_ts),
                    OffsetDateTime::from_unix_timestamp(to_ts),
                ) {
                    (Ok(from), Ok(to)) => ((from, to)),
                    _ => default,
                }
            }
        }
    };

    eprintln!("Looking up changes from {} to {}", from, to);

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

    changes.retain(|(_, v)| *v >= args.min);
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
