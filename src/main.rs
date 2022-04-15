extern crate git_heat;
use std::{collections::HashMap, path::PathBuf};

use clap::Parser;
use git2::Repository;
use git_heat::*;
use time::OffsetDateTime;

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
}

fn main() {
    let args = Args::parse();

    let repo = Repository::open(args.repo).expect("not a valid git repo");

    let format = time::format_description::parse("[year]-[month]-[day]").unwrap();

    let from = OffsetDateTime::parse(&args.from, &format).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let to = OffsetDateTime::parse(&args.to, &format).unwrap_or_else(|_| OffsetDateTime::now_utc());

    let commits =
        commits_in_date_range(from, to, &repo)
            .expect("unable to retrieve commits in date range");
    let pairs = pair_commits(commits);
    let diffs = pairs.map(|(new, old)| {
        get_diff_of_commits(old, new, &repo).expect("unable to get diff from commits")
    });

    let mut renames = HashMap::new();
    let changes = diffs.map(|diff| get_files_changed(diff, &mut renames));

    let changes = changes
        .reduce(|mut acc, changes| {
            for (k, v) in changes.into_iter() {
                *acc.entry(k).or_default() += v;
            }
            acc
        })
        .unwrap_or_default();

    println!("{:?}", changes);
}
