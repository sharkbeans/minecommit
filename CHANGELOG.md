# Changelog

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
