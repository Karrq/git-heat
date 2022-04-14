extern crate git_heat;
use std::collections::HashMap;

use git2::Repository;
use git_heat::*;
use time::OffsetDateTime;

//TODO:
// CLI: repository path
//      date range
fn main() {
    let repo = Repository::open(".").expect("not a valid git repo");

    let commits =
        commits_in_date_range(OffsetDateTime::UNIX_EPOCH, OffsetDateTime::now_utc(), &repo)
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
