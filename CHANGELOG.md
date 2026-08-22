# Changelog

## 0.5.0 (2026-08-22)

The dashboard has been rebuilt around what a player is trying to do, rather
than around the Git operations underneath.

Breaking change

- "Create backup" and "Upload" are now one button. Keeping them apart was the
  most confusing thing about MineCommit: backing up without uploading left the
  world safe on one computer while the player believed it was everywhere. One
  action now records the world and sends it to the cloud, and reports the two
  halves separately if only the upload fails -- in which case the world is
  still safely backed up and the next upload carries it along.

New features

- A single screen: worlds down the left, and for the selected world one card
  saying what to do about it. The card reads the world's state and offers
  exactly one action, so there is never a question of which button to press or
  in which order.
- World history with timestamps, showing which computer each backup came from
  and letting any point be restored. Backups made on this computer are labelled
  "this PC" rather than by hostname.
- Worlds are added by ticking them off a list of the worlds actually in the
  saves folder, instead of typing a path. The folder defaults to
  %APPDATA%\.minecraft\saves on Windows, ~/.minecraft/saves on Linux and
  ~/Library/Application Support/minecraft/saves on macOS, and can be changed in
  settings.
- A world that Minecraft still has open is reported as such before the backup
  is attempted, using the same session.lock check the backup itself performs.
- A world that has not been played since its last backup is uploaded without
  recording a second, identical entry in its history.
- Settings moved into a dialog reachable from the top bar, alongside the cloud
  repository the selected world is connected to and whether it can be reached.

Removed

- The dock, the sidebar, the separate worlds page and the separate settings
  page, along with the Commit, Push, Pull and Restore dialogs they opened.
  Everything they did is on the dashboard.

## 0.4.11 (2026-08-22)

Bug fixes

- Stop flashing console windows on Windows. `git.exe` is a console
  application, so Windows opened a black window for every invocation the GUI
  made directly — checking a repository, listing branches, reading the commit
  author — and a screen full of them appearing and vanishing looked like
  malware. The core already suppressed this for the commands it runs itself;
  now every Git invocation in the workspace goes through one constructor that
  sets `CREATE_NO_WINDOW`, and a test fails if a new one does not.

## 0.4.10 (2026-08-21)

New features

- Add "Download from cloud" for a computer that does not have the world yet.
  Previously a world could only be added by selecting a folder that already
  contained level.dat, so a second computer had no way to obtain a world that
  existed only in the cloud, and had to copy it across by hand first.
- Choose the cloud branch from the branches the repository actually has,
  instead of typing one. A branch that did not match silently resolved to
  "no backups yet" rather than reporting anything wrong.

Bug fixes

- Forget the previous repository's tracking refs when a world is pointed at a
  different cloud repository, and drop the tracking ref for a branch that no
  longer exists remotely. Cloud status is answered from cached refs without
  contacting the network, so a deleted repository could keep reporting state,
  and a fetch of a missing branch was treated as success while leaving the
  stale ref in place.

## 0.4.9 (2026-08-21)

Bug fixes

- Back up worlds on Windows again. MineCommit holds the world's session.lock
  for the duration of a backup to prove Minecraft is not running, then tried to
  read that same file as part of the backup. Windows byte-range locks are
  mandatory, so its own read failed with "another process has locked a portion
  of the file (os error 33)". Unix locks are advisory, so this never appeared
  on Linux.
- Stop storing session.lock altogether. It is ephemeral state that Minecraft
  rewrites whenever a world is opened and recreates when absent, so keeping it
  only added a spurious change to every backup.

## 0.4.8 (2026-08-21)

Bug fixes

- Restore over an existing world on Windows. MineCommit held the world's own
  session.lock open while renaming the save directory aside, and Windows
  refuses to rename a directory that still has a handle open inside it. The
  restore failed with "Access is denied. (os error 5)" and rolled back.
- Run the test suite on Windows as well as Linux, since the bug above could
  not be reproduced on Linux at all.

## 0.4.7 (2026-08-21)

Bug fixes

- Report a local branch that is ahead of the cloud as `LocalAhead` again. Two
  match arms bound `local` and `remote` in reverse order, so the guard repeated
  the preceding `RemoteAhead` check and could never be true. Every backup made
  after the first successful upload was reported as `Diverged`, which hid the
  GUI's **Upload** button and made `push` refuse to run.
- Back up worlds whose chunks predate the Minecraft 1.18 layout instead of
  failing with `missing 'Status' field in chunk nbt`. The flattener only
  understands 1.18+ chunks, which keep `Status` and `sections` at the NBT root;
  1.13-1.17 worlds nest them under `Level` and older worlds have no `Status` at
  all. Such region files are now stored byte-for-byte and restored unchanged.
  Note that they are not deduplicated, so their backups are larger.
- Preserve `level.dat_mcr`, the pre-Anvil level data Minecraft leaves behind
  when converting a world, rather than reporting it as an unhandled file.
- Match `session.lock` anywhere in a save instead of only at its root. Maps are
  sometimes distributed with a second save nested inside them, and the nested
  lock file was reported as unhandled, which failed the whole backup.
- Fall back to a plain `git repack` when Git rejects `--path-walk`. The option
  only exists in recent Git builds, and repacking previously failed on the Git
  shipped by most distributions.

Internal

- Unify the workspace, GUI and Tauri bundle versions, which had drifted to
  0.3.0 and 0.1.0 and no longer matched the released tags.
- Run `cargo test` on push and pull request, and require it to pass before a
  release is created. Only `cargo build` ran previously, so an existing test
  covering the sync regression above never executed.

## 0.2.0 (2026-06-06)

BREAKING CHANGE

- b8cf2e9 Fix recipe book sort condition
- af592d6 Sort player attributes
- fd621fd Move entity Motion, Pos, Rotation fields to region top
- 5fa03f9 Move entity Motion, Pos, Rotation fields to chunk top
- 4e0e4be Sort entity attributes by id
- a63b18f Sort entities nbt
- 200ad99 Collect entities in single file
- 2489270 Collect other nbts into single file
- 0399081 Collect InhabitedTime & LastUpdate into timestamp file
- 55987dd Store chunk region timestamp header in nbt

New Features

- 92b40dd Replace sort with sort_unstable
- ff1e5ad More precision when cli print result

Bug fixes

- Fix recipe book sort condition
