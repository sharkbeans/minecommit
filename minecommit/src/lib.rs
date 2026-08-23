use std::{collections::HashSet, path::PathBuf};

use anyhow::{Context, Result};

use crate::{
    handler::{CrafterImpl, Handler},
    odb::{LocalFsOdb, LocalGitOdb, OdbReader},
    utils::cmd::{exec, git_cmd},
};

mod handler;
pub mod odb;
pub mod progress;
pub mod sync;
pub mod utils;

#[derive(Debug, Clone)]
pub struct Config {
    save_dir: PathBuf,
    storage_dir: PathBuf,
    extra_patterns: Vec<String>,
    ignore_patterns: Vec<String>,
}

impl Config {
    pub fn new(
        save_dir: PathBuf,
        storage_dir: PathBuf,
        extra_patterns: Vec<String>,
        ignore_patterns: Vec<String>,
    ) -> Self {
        Self {
            save_dir,
            storage_dir,
            extra_patterns,
            ignore_patterns,
        }
    }
    pub fn flatten(self) -> Result<()> {
        let save = LocalFsOdb::from_dir(self.save_dir.to_owned());
        let mut repo = LocalFsOdb::from_dir(self.storage_dir.to_owned());

        for crafter in CrafterImpl::get_crafters(self.extra_patterns, self.ignore_patterns) {
            crafter.flatten(&save, &mut repo)?;
        }

        Ok(())
    }

    pub fn unflatten(self) -> Result<()> {
        let mut save = LocalFsOdb::from_dir(self.save_dir.to_owned());
        let repo = LocalFsOdb::from_dir(self.storage_dir.to_owned());

        for crafter in CrafterImpl::get_crafters(self.extra_patterns, self.ignore_patterns) {
            crafter.unflatten(&mut save, &repo)?;
        }

        Ok(())
    }

    pub fn commit(
        self,
        parents: Vec<String>,
        message: &str,
        r#ref: Option<String>,
        author_name: Option<&str>,
        author_email: Option<&str>,
    ) -> Result<Vec<String>> {
        let save = LocalFsOdb::from_dir(self.save_dir.to_owned());
        let mut git = if let Some(from) = parents.first() {
            LocalGitOdb::from_commit(self.storage_dir.to_owned(), from.clone())
        } else {
            LocalGitOdb::new(self.storage_dir.to_owned())
        }?;

        // Counted before any work starts so the bar has a denominator. This
        // walks the save directory a second time, which costs seconds on a
        // world whose backup costs minutes.
        progress::begin(save.glob("**/*")?.len() as u64);

        let mut processed = HashSet::new();
        for crafter in CrafterImpl::get_crafters(self.extra_patterns, self.ignore_patterns) {
            processed.extend(crafter.flatten(&save, &mut git)?);
        }
        progress::end();

        let unprocessed = save
            .glob("**/*")?
            .into_iter()
            .filter(|item| !processed.contains(item))
            .collect::<Vec<_>>();

        // Do not make a partial backup look successful. Flattening may have
        // written loose Git objects, but without a commit or ref update they
        // are unreachable and no backup history advances.
        if !unprocessed.is_empty() {
            progress::end();
            log::warn!(
                "Found {} unhandled files; leaving the backup branch unchanged",
                unprocessed.len()
            );
            return Ok(unprocessed);
        }

        let commit = git.commit(parents.as_slice(), message, author_name, author_email)?;

        if let Some(r#ref) = r#ref {
            let cmd = git_cmd(self.storage_dir.to_owned(), ["update-ref", &r#ref, &commit]);
            exec(cmd, None).context("failed to run update-ref")?;
            log::info!("{:?} -> {commit}", r#ref);
        } else {
            log::warn!("Dangling commit {commit}");
        }
        Ok(unprocessed)
    }

    pub fn checkout(self, commit: String) -> Result<()> {
        let mut save = LocalFsOdb::from_dir(self.save_dir.to_owned());
        let git = LocalGitOdb::from_commit(self.storage_dir.to_owned(), commit)?;

        for crafter in CrafterImpl::get_crafters(self.extra_patterns, self.ignore_patterns) {
            crafter.unflatten(&mut save, &git)?;
        }

        Ok(())
    }
}
