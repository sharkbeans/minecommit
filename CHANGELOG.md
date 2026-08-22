# Changelog

## 0.8.1 (2026-08-22)

Bug fixes

- Back up worlds that contain a `.dat` file which is not NBT. MineCommit
  claims every `.dat` in a save, and mods put files there that were never NBT
  -- Lunar Client's minimap tiles, for one. The whole backup ended with
  "nbt data is empty" over a file the player did not know existed. Such files
  are now stored byte for byte and restored unchanged.

## 0.8.0 (2026-08-22)

Breaking change

- Sign in through a GitHub App that reaches only the repositories you choose,
  rather than an OAuth app asking for every repository you own. The old
  consent screen asked for access to all public and private repositories,
  which is an alarming thing for a Minecraft backup tool to want and far more
  than it needs. MineCommit now sees the repositories you tick when installing
  it, and within those only their contents. Signing in again is required.
- MineCommit no longer creates a repository for you. Doing that needs rights
  over the whole account, which is the breadth this change exists to remove.
  Connecting a world instead offers a link to create one on GitHub and a link
  to choose which repositories MineCommit may use, then lists what you granted.

New features

- Connect a world by picking from the repositories you have granted, instead
  of pasting an address. The list is exactly what MineCommit can reach, so
  there is nothing to copy and nothing to get wrong.

Internal

- The GitHub App's client ID and slug are compiled in from
  MINECOMMIT_GITHUB_CLIENT_ID and MINECOMMIT_GITHUB_APP_SLUG. Both are public;
  a build missing either refuses to sign in and says so.
- Read the access token back from Git's credential store when it is needed, so
  the repository list still works after a restart.

## 0.7.0 (2026-08-22)

Breaking change

- Sign in with GitHub instead of pasting an access token. GitHub shows a short
  code, you enter it on github.com, and MineCommit waits: nothing is typed into
  MineCommit and no password or token passes through it. Anyone who signed in
  on 0.6.0 stays signed in; the token they pasted is still in Git's credential
  store and still works.

New features

- Show that the app is loading its worlds when it starts. The world list was
  empty and the panel beside it read "add a world to start backing it up" until
  the worlds had been read, which said the opposite of the truth to anyone with
  worlds already set up.

Bug fixes

- Replace the signed-out account menu with a plain "Sign in with GitHub"
  button. A menu that opened to reveal a single item added a click and a place
  to get stuck for no benefit.

Internal

- The GitHub OAuth client ID is compiled in from MINECOMMIT_GITHUB_CLIENT_ID.
  It is public by design: the device flow exists for clients that cannot keep a
  secret. A build without one refuses to sign in and says so, rather than
  failing obscurely at GitHub.

## 0.6.0 (2026-08-22)

New features

- Sign in to GitHub from MineCommit, and sign out again, from an account menu
  in the top right. Until now the app had no idea who you were: cloud access
  rode entirely on whatever Git's credential helper happened to hold, and
  nothing in the interface said whether that would work.
- Have MineCommit create the backup repository for you. Signed in, connecting a
  world offers a private repository named after it, made on GitHub and
  connected in one step, instead of sending you off to create one by hand and
  come back with the address. Pointing at a repository you already have is
  still offered alongside it.
- Adding a world now leads straight into connecting it, rather than adding it
  and leaving you to find the button. Signing in from that flow returns to it.
- Back up a world without any cloud at all. The backup stays on this computer,
  which is enough to undo a creeper, and the world can be connected later.
- Put a second world in the repository the first one already uses, chosen from
  a list, rather than creating a repository per world. One repository holding a
  branch per world was always how MineCommit worked; nothing in the interface
  said so.

Bug fixes

- Create a world's backup repository when the world is added. Adding a world
  recorded where its backups would live without ever creating it, so the world
  reported "is not a bare Git repository", could not be backed up or connected,
  and no screen offered to create one. Worlds already added in that state are
  repaired when they are next opened.

Notes on the sign-in

- MineCommit asks for a GitHub access token rather than your password, and the
  "Get a token" button opens GitHub with the right settings already filled in.
  The token goes to Git's own credential store -- the Credential Manager on
  Windows, libsecret where it is available on Linux -- so pushes authenticate
  without asking again. MineCommit never writes it to its own files. Signing
  out removes it.
- An existing credential helper is always left alone; one is only configured
  if you have none.

## 0.5.1 (2026-08-22)

Bug fixes

- Look up the branches of a cloud repository again. "Add a world" passed the
  repository address under the wrong name, so the lookup failed with
  "invalid args `remoteUrl` for command `list_remote_branches`" and a world
  could not be downloaded from the cloud at all.
- Build the path for a downloaded world with the separator its saves folder
  already uses, instead of always a forward slash.

Internal

- Check that every `invoke` in the frontend passes exactly the arguments its
  Rust command declares, and run it in CI before a release is created. Tauri
  resolves these by name at runtime, so the bug above type-checked, built and
  shipped without anything noticing.
- Cover the world scan behind "Add a world": only folders holding level.dat
  are offered, and a saves folder that is not there is reported rather than
  read as empty.

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
