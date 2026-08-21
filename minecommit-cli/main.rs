use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use minecommit::{
    Config,
    sync::{
        backup_message_with_device, lock_inactive_world, restore_commit, RemoteSync, DEFAULT_BRANCH,
    },
    utils::cmd::{git_cmd, git_count_objects, git_repack, git_repo_exists},
};

/// MineCommit - Commit your Minecraft world to Git
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(flatten)]
    verbosity: Verbosity<InfoLevel>,
    #[command(subcommand)]
    action: CliSubcommand,
}

#[derive(Subcommand)]
enum CliSubcommand {
    /// Flatten save to the repo dir
    Flatten {
        /// Path to your save
        save_dir: PathBuf,
        /// Path to the flatten Git repository
        repo_dir: PathBuf,
    },
    /// Restore save from repo dir
    Unflatten {
        /// Path to your save
        save_dir: PathBuf,
        /// Path to the flatten Git repository
        repo_dir: PathBuf,
    },
    /// Flatten save and commit to Git
    Commit {
        /// Path to your save
        save_dir: PathBuf,
        /// Path to the bare Git repository
        #[arg(value_parser = git_repo_exists)]
        git_dir: PathBuf,
        /// Commit to this branch.
        #[arg(short, long, default_value = DEFAULT_BRANCH)]
        branch: String,
        /// Commit as initial commit.
        #[arg(long)]
        init: bool,
        /// Commit message.
        #[arg(short, long)]
        message: String,
        /// Automatically repack loose objects.
        #[arg(long = "repack", default_value_t = false)]
        use_repack: bool,
        /// Glob patterns to additionally include
        #[arg(short = 'p', long)]
        extra_patterns: Vec<String>,
        /// Glob patterns to explicit ignore
        #[arg(short = 'i', long)]
        ignore_patterns: Vec<String>,
    },
    /// Restore save from commit
    Checkout {
        /// Path to your save
        save_dir: PathBuf,
        /// Path to the bare Git repository
        git_dir: PathBuf,
        /// Commit-ish to checkout (commit ID or revision expression, e.g. HEAD^1, branch~2)
        #[arg(short, long)]
        commit: String,
    },
    /// Configure and inspect the cloud Git remote for a bare MineCommit repository
    Remote {
        #[command(subcommand)]
        action: RemoteSubcommand,
    },
    /// Fetch the configured cloud branch without changing the local Minecraft world
    Fetch {
        /// Path to the bare Git repository
        #[arg(value_parser = git_repo_exists)]
        git_dir: PathBuf,
        /// Branch to fetch
        #[arg(short, long, default_value = DEFAULT_BRANCH)]
        branch: String,
    },
    /// Safely push local MineCommit backups after fetching and checking ancestry
    Push {
        /// Path to the bare Git repository
        #[arg(value_parser = git_repo_exists)]
        git_dir: PathBuf,
        /// Branch to push
        #[arg(short, long, default_value = DEFAULT_BRANCH)]
        branch: String,
    },
    /// Fetch, fast-forward when safe, and restore a newer cloud backup
    Pull {
        /// Path to the local Minecraft save
        save_dir: PathBuf,
        /// Path to the bare Git repository
        #[arg(value_parser = git_repo_exists)]
        git_dir: PathBuf,
        /// Branch to synchronize
        #[arg(short, long, default_value = DEFAULT_BRANCH)]
        branch: String,
    },
    /// Synchronize before playing: fetch, safely restore remote changes, and never merge
    Sync {
        /// Path to the local Minecraft save
        save_dir: PathBuf,
        /// Path to the bare Git repository
        #[arg(value_parser = git_repo_exists)]
        git_dir: PathBuf,
        /// Branch to synchronize
        #[arg(short, long, default_value = DEFAULT_BRANCH)]
        branch: String,
    },
    /// Utility tools for debug
    Utils {
        #[command(subcommand)]
        action: UtilsSubcommand,
    },
}

#[derive(Subcommand)]
enum RemoteSubcommand {
    /// Associate this MineCommit repository with a provider-agnostic Git remote URL
    Add {
        /// Path to the bare Git repository
        #[arg(value_parser = git_repo_exists)]
        git_dir: PathBuf,
        /// HTTPS, SSH, or other Git remote URL
        url: String,
        /// Branch MineCommit will synchronize (defaults to main)
        #[arg(short, long, default_value = DEFAULT_BRANCH)]
        branch: String,
    },
    /// Show the last fetched relationship between local and cloud backups
    Status {
        /// Path to the bare Git repository
        #[arg(value_parser = git_repo_exists)]
        git_dir: PathBuf,
        /// Branch to inspect
        #[arg(short, long, default_value = DEFAULT_BRANCH)]
        branch: String,
    },
}

fn print_remote_status(status: &minecommit::sync::RemoteStatus) {
    println!("Cloud status: {}", status.state.description());
    println!(
        "Remote: {}",
        status.remote_url.as_deref().unwrap_or("(not configured)")
    );
    println!("Branch: {}", status.branch);
    println!(
        "Local backup: {} ({}, {})",
        status.local_commit.as_deref().unwrap_or("(none)"),
        status.local_timestamp.as_deref().unwrap_or("unknown time"),
        status.local_device.as_deref().unwrap_or("unknown device"),
    );
    println!(
        "Cloud backup: {} ({}, {})",
        status.remote_commit.as_deref().unwrap_or("(not fetched or empty)"),
        status.remote_timestamp.as_deref().unwrap_or("unknown time"),
        status.remote_device.as_deref().unwrap_or("unknown device"),
    );
}

#[derive(Subcommand)]
enum UtilsSubcommand {
    /// Dump chunk nbt data to stdout
    Chunk {
        /// Path to region file
        region_path: PathBuf,
        /// Chunk X
        chunk_x: i32,
        /// Chunk Z
        chunk_z: i32,
    },
}

fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbosity.log_level_filter())
        .init();

    match cli.action {
        CliSubcommand::Flatten { save_dir, repo_dir } => {
            Config::new(save_dir, repo_dir, vec![], vec![]).flatten()
        }
        CliSubcommand::Unflatten { save_dir, repo_dir } => {
            Config::new(save_dir, repo_dir, vec![], vec![]).unflatten()
        }
        CliSubcommand::Commit {
            save_dir,
            git_dir,
            branch,
            init,
            message,
            use_repack,
            extra_patterns,
            ignore_patterns,
        } => {
            // Hold Minecraft's session lock for the full read/commit operation.
            // A stale lock file is fine; an actively held lock is not.
            let _world_lock = lock_inactive_world(&save_dir)?;
            let parents = {
                let mut cmd = git_cmd(&git_dir, ["rev-parse", &format!("{branch}^{{commit}}")]);
                let out = cmd.output().context("failed to run git rev-parse")?;
                let branch_exists = out.status.success();
                match (branch_exists, init) {
                    (true, false) => {
                        vec![
                            String::from_utf8(out.stdout)
                                .context("git output is not valid UTF-8")?
                                .trim()
                                .to_owned(),
                        ]
                    }
                    (false, true) => vec![],
                    (true, true) => anyhow::bail!("branch '{branch}' exists, remove --init"),
                    (false, false) => anyhow::bail!(
                        "invalid branch name '{branch}'. Self-check via 'git --git-dir {:?} rev-parse {branch}^{{commit}}'",
                        git_dir.as_os_str()
                    ),
                }
            };
            let r#ref = format!("refs/heads/{}", &branch);

            let size_before = git_count_objects(&git_dir)
                .context("failed to count git objects")?
                .total_size_mib();
            let unprocessed = Config::new(
                save_dir,
                git_dir.to_owned(),
                extra_patterns,
                ignore_patterns,
            )
            .commit(
                parents,
                &backup_message_with_device(&message),
                Some(r#ref),
                None,
                None,
            )?;
            if !unprocessed.is_empty() {
                for item in &unprocessed {
                    log::error!("Skipped file: {item}");
                }
                anyhow::bail!(
                    "Skipped {} files because they are not caught by any handler. Catch them via -p or ignore them via -i.",
                    unprocessed.len()
                );
            }

            if use_repack {
                git_count_objects(&git_dir).context("failed to count git objects")?;
                git_repack(&git_dir)?;
            } else {
                log::warn!("--repack is not enabled, Git repository can get bloated")
            }

            let size_after = git_count_objects(&git_dir)
                .context("failed to count git objects")?
                .total_size_mib();
            log::info!(
                "Done. Repo total size: {size_after:.3} MiB (up {:+.4}% from {size_before:.3} MiB)",
                (size_after - size_before) / size_before * 100.0
            );
            Ok(())
        }
        CliSubcommand::Checkout {
            save_dir,
            git_dir,
            commit,
        } => {
            let result = restore_commit(&save_dir, &git_dir, &commit)?;
            if let Some(backup) = result.backup_path {
                log::info!("Existing save preserved at {backup:?}");
            }
            log::info!("Done");
            Ok(())
        }

        CliSubcommand::Remote { action } => match action {
            RemoteSubcommand::Add {
                git_dir,
                url,
                branch,
            } => {
                let sync = RemoteSync::new(git_dir, branch)?;
                sync.configure_remote(&url)?;
                print_remote_status(&sync.status()?);
                Ok(())
            }
            RemoteSubcommand::Status { git_dir, branch } => {
                let sync = RemoteSync::new(git_dir, branch)?;
                print_remote_status(&sync.status()?);
                Ok(())
            }
        },

        CliSubcommand::Fetch { git_dir, branch } => {
            let sync = RemoteSync::new(git_dir, branch)?;
            sync.fetch()?;
            print_remote_status(&sync.status()?);
            Ok(())
        }

        CliSubcommand::Push { git_dir, branch } => {
            let sync = RemoteSync::new(git_dir, branch)?;
            let status = sync.push()?;
            print_remote_status(&status);
            Ok(())
        }

        CliSubcommand::Pull {
            save_dir,
            git_dir,
            branch,
        }
        | CliSubcommand::Sync {
            save_dir,
            git_dir,
            branch,
        } => {
            let sync = RemoteSync::new(git_dir, branch)?;
            let result = sync.sync_before_playing(save_dir)?;
            if result.restored {
                if let Some(backup) = result.backup_path {
                    log::info!("Existing save preserved at {backup:?}");
                }
                log::info!("Cloud backup was safely restored");
            }
            print_remote_status(&result.status);
            Ok(())
        }

        CliSubcommand::Utils { action } => {
            let _: () = match action {
                UtilsSubcommand::Chunk {
                    region_path,
                    chunk_x,
                    chunk_z,
                } => {
                    use minecommit::utils::region::{parse_xz, read_region};
                    use std::fs;
                    use std::io::{self, Write};

                    let (region_x, region_z) = parse_xz(
                        region_path
                            .file_name()
                            .context("invalid region path")?
                            .to_str()
                            .context("region path contains invalid UTF-8")?,
                    )
                    .context("failed to parse region filename")?;
                    let (_, xz_nbts) = read_region(
                        fs::File::open(region_path).context("failed to open region file")?,
                        region_x,
                        region_z,
                    )
                    .context("failed to read region file")?
                    .context("region file is empty")?;
                    let (_, _, nbt) = xz_nbts
                        .iter()
                        .find(|(x, z, _)| *x == chunk_x && *z == chunk_z)
                        .with_context(|| {
                            format!(
                                "missing chunk, all chunk positions: {:#?}",
                                xz_nbts
                                    .iter()
                                    .map(|(x, z, _)| format!("({x}, {z})"))
                                    .collect::<Vec<_>>()
                            )
                        })
                        .context("chunk not found")?;
                    io::stdout()
                        .write_all(nbt)
                        .context("failed to write to stdout")?;
                }
            };
            Ok(())
        }
    }
}
