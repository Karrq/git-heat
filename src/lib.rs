use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use git2::{Commit, Diff, DiffOptions, Repository, Time};
use itertools::Itertools;
use snafu::{Backtrace, Snafu};
use time::{OffsetDateTime, UtcOffset};

use rayon::prelude::*;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("error from libgit2: {source}"), context(false))]
    Git2 {
        backtrace: Backtrace,
        source: git2::Error,
    },

    #[snafu(
        display("unable to build time's component with git2's commit time"),
        context(false)
    )]
    TimeOutOfRange {
        backtrace: Backtrace,
        source: time::error::ComponentRange,
    },
}

pub type Result<T> = core::result::Result<T, Error>;

fn git2_time_to_offset(time: Time) -> Result<OffsetDateTime> {
    let since_epoch = time.seconds();
    let offset = time.offset_minutes();

    let datetime = OffsetDateTime::from_unix_timestamp(since_epoch)?;
    let offset = UtcOffset::from_whole_seconds(offset * 60)?;

    Ok(datetime.to_offset(offset))
}

/// Returns a list of commits within the indicated date range
pub fn commits_in_date_range<'repo>(
    mut from: OffsetDateTime,
    mut to: OffsetDateTime,
    repo: &'repo Repository,
) -> Result<impl Iterator<Item = Commit<'repo>>> {
    //make sure datetimes are in UTC
    from = from.to_offset(UtcOffset::UTC);
    to = to.to_offset(UtcOffset::UTC);

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?; //start from the HEAD
    revwalk.set_sorting(git2::Sort::TIME.union(git2::Sort::TOPOLOGICAL))?; //sort by TIME, children first

    let commits = revwalk
        //discard all Oids that return error... why would they anyways?
        .filter_map(core::result::Result::ok)
        // get all the commits with this Oid.. discard errors, again why would it error?
        .filter_map(|oid| repo.find_commit(oid).ok())
        .filter_map(|commit| {
            git2_time_to_offset(commit.time())
                .ok() //not interested in commits that have weird times.
                .map(|time| (commit, time))
        })
        .filter(move |(_, time)| {
            //make sure time is within range
            &from <= time && time <= &to
        })
        //discard time
        .map(|(commit, _)| commit);

    Ok(commits)
}

/// Will pair commits together in a chain
///
/// ```rust,ignore
/// let commits = [a, b, c];
///
/// let paired = pair_commits([a, b, c].iter());
///
/// assert_eq!(paired, [(a, Some(b)), (b, Some(c)), (c, None)])
/// ```
pub fn pair_commits<'repo>(
    commits: impl Iterator<Item = Commit<'repo>>,
) -> impl Iterator<Item = (Commit<'repo>, Option<Commit<'repo>>)> {
    let peekable = itertools::peek_nth(commits);
    peekable.batching(|it| match it.next() {
        None => None,
        Some(new) => match it.peek() {
            Some(old) => Some((new, Some(old.clone()))),
            None => Some((new, None)),
        },
    })
}

/// Get the diff of the changes between these 2 commits
///
/// old is optional in case there's no parent commit
pub fn get_diff_of_commits<'repo>(
    old: Option<Commit<'repo>>,
    new: Commit<'repo>,
    repo: &'repo Repository,
) -> Result<Diff<'repo>> {
    //if no "old" commit was provided then the tree
    // is fine to be None as it will be considered an empty tree
    // so all files in "new" will be "added"
    let old = old.map(|c| c.tree()).transpose()?;
    let new = new.tree()?;

    let mut diffopt = DiffOptions::default();
    diffopt
        //add typechanges (files to trees)
        .include_typechange(true)
        .include_typechange_trees(true)
        //skip granular hunk and data checks, we don't care about
        // the specific diff, just that the file was touched
        .skip_binary_check(true)
        //ignore formatting changes
        .ignore_whitespace(true)
        .ignore_whitespace_eol(true)
        .ignore_whitespace_change(true)
        .ignore_blank_lines(true);

    repo.diff_tree_to_tree(old.as_ref(), Some(&new), Some(&mut diffopt))
        .map_err(Into::into)
}

/// Retrieve a list of changed files and the number of changes per file
///
/// `renames` is a map for file renames, so the resulting map always contains the newest name
pub fn get_files_changed<'repo>(
    diff: Diff<'repo>,
    renames: &mut HashMap<PathBuf, PathBuf>,
) -> HashMap<PathBuf, u32> {
    let mut changes = HashMap::new();

    fn process_delta<'c, 'r, 'repo>(
        delta: git2::DiffDelta<'repo>,
        renames: &'r mut HashMap<PathBuf, PathBuf>,
        changes: &'c mut HashMap<PathBuf, u32>,
    ) {
        use git2::Delta;

        let old = delta.old_file().path().map(ToOwned::to_owned);
        let new = delta.new_file().path().map(ToOwned::to_owned);

        let filename = match delta.status() {
            Delta::Unmodified
            | Delta::Ignored
            | Delta::Untracked
            | Delta::Unreadable
            | Delta::Conflicted => None, //ignore these changes
            Delta::Added | Delta::Copied => Some(new.unwrap()),
            //new would be a None since it doesn't exist anymore
            Delta::Deleted => Some(old.unwrap()),
            //if renamed or typechange then store the rename
            Delta::Renamed | Delta::Typechange => {
                //store filename change in renames
                // since it's a rename neiter paths can be empty
                let new = new.unwrap();
                renames.insert(old.unwrap(), new.clone());
                Some(new)
            }
            Delta::Modified => {
                //lookup in the map recursively for the newest name
                // this is because we are interested in visualizing
                // the most changed files, but if we don't track the renames
                // we risk looking at "old" files that were later renamed
                // so the renames should "carry" over the changes
                let mut filename = old.unwrap();
                while let Some(renamed) = renames.get(&filename) {
                    filename = renamed.clone();
                }

                Some(filename)
            }
        };

        if let Some(filename) = filename {
            *changes.entry(filename).or_default() += 1;
        }
    }

    diff.foreach(
        &mut |delta, _| {
            process_delta(delta, renames, &mut changes);
            true
        } as _,
        None,
        None,
        None,
    );

    changes
}
