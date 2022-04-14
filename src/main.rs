extern crate git_heat;
use std::collections::HashMap;

use git_heat::*;


fn main() {
    let repo = Repository::open(".").expect("not a valid git repo");

    let commits = commits_in_date_range(OffsetDateTime::UNIX_EPOCH, OffsetDateTime::now_utc(), &repo);
    let pairs = pair_commits(commits);
    let diffs = pairs.map(|(new, old)| get_diff_of_commits(old, new, &repo));

    let mut renames = HashMap::new();
    let changes = diffs.map(|diff| get_files_changes(diff, &mut renames));

    let changes = changes.fold(HashMap::new(), |acc, changes| {
        acc.extend(changes.into_iter());
        acc
    });

    println!("{:?}", changes);
}
