# Changelog

## 0.12.2 (2026-08-24)

Fixes

- MineCommit could not find the copies it makes. Every restore keeps the world
  it replaced, and where those are kept has changed three times over the
  releases; the search for them only ever looked in the oldest of the three
  places. So since 0.11 the app has been making copies it never afterwards
  mentioned. One machine had 9.3 GB of them, in four copies across two folders.

  All three places are searched now. The banner leads with how much room they
  take, and says separately how many are sitting among your worlds where
  Minecraft lists them.

- A fetch nobody was waiting on moved the progress bar. Checking whether the
  cloud has anything new is a fetch too, and so is the one an upload does before
  it starts, and both were quietly driving a bar.

- Three checks that read this program's own source to make sure it keeps a
  promise did not work on Windows, where the repository is checked out with
  different line endings. One failed outright; two found nothing to check and
  passed, which is worse.

## 0.12.1 (2026-08-24)

Fixes

- A restore of a 3.1 GB world said "30 / 81 GB". Both numbers were real and
  neither meant anything: a world is stored one chunk at a time and
  uncompressed, so Git can tell which chunks changed, and 505,862 stored pieces
  come to 81 GB even though the same world is 3.1 GB in the saves folder and
  1.9 GB as a backup. A restore now counts pieces, and says why there are so
  many of them.

  Backing up still shows a size, because there it measures the world folder as
  it sits on disk. So does uploading or downloading, which measures what
  actually crosses the network.

- The elapsed time counted the hours the computer spent asleep. A laptop
  suspended part-way through a restore came back reporting 167 minutes of work
  for about ten minutes of it, and predicted 4.9 hours still to go. The clock
  now stops when the machine does. It also no longer depends on the window
  being on screen, which it was, and which is exactly where a long backup is
  left to run.

- An upload or download left its progress bar running after it finished, and
  handed it to whatever ran next.

- Times over an hour are shown as hours. "167m 43s" was arithmetic homework.

## 0.12.0 (2026-08-24)

Changes

- Each world now shows what Minecraft itself knows about it: the version it was
  last opened in, the game mode, the difficulty, whether it is hardcore, the
  in-game day, its size on disk, the spawn point and the seed. The list of
  worlds to add shows the version too, which is what tells two similarly named
  folders apart.

  Reading that needed two layouts. The 26.x releases moved difficulty, hardcore
  and the spawn point into sections of their own, turned the difficulty from a
  number into a name, and took the seed out of level.dat altogether -- so
  against a current world the old field names found nothing at all, and reported
  a hardcore world as not hardcore.

- The progress bar counts bytes rather than files, and says "351 / 1,024 MB". A
  world is a few hundred large region files next to a few thousand tiny ones, so
  a bar counting files sprinted through the small ones and then sat still.

- Uploads and downloads have a bar at all for the first time. Git knows exactly
  how a transfer is going but only draws it to a terminal; it is now asked to
  report anyway and its own figures are shown. The card names what it is doing
  -- reading, rebuilding, downloading, uploading -- and estimates the time left
  from the current step alone rather than from everything before it.

- A world keeps the name it was first backed up under, on every computer that
  gets it. The two places that let you type a different one are gone: renaming a
  world on the way down left the same world existing twice under two names, with
  no backup history connecting them and nothing able to tell they were the same.

- There is a Minecraft block beside each world. It is drawn in the app rather
  than taken from anywhere -- Minecraft's own textures are Mojang's.

Fixes

- A world containing a file whose name is not plain ASCII -- a data pack called
  "café", a folder with a space or a quotation mark in it -- restored that file
  under a mangled name, and the real one did not come back at all. Git escapes
  such names when listing what a backup holds, and the escaped form was being
  read back as though it were the name.

- Very large world seeds were shown wrong. A seed uses the whole 64-bit range
  and does not survive the only kind of number the interface can carry; the seed
  -352129843062846360 was displayed as -352129843062846400, which generates an
  entirely different world.

## 0.11.2 (2026-08-23)

Changes

- Worlds are no longer re-checked on a timer. The button in the top right still
  checks all of them whenever you want it to; nothing happens on its own any
  more.

  The fault that made the timer dangerous was fixed in 0.11.1, but the shape of
  it is what turned a small mistake into a recurring one: a background loop
  reaching into save folders every few minutes is a standing bet that everything
  it does there is harmless. Asking on request costs a button press and makes no
  such bet.

Safety

An audit of every place MineCommit moves or erases a directory, and what each
could be pointed at by a path that was wrong. None of these had been reported;
all of them could have cost somebody a world.

- A world named like one of MineCommit's own copies -- "Backup.snapshot", say --
  was hidden from the world list and then offered up for deletion as clutter. A
  copy is now recognised by the timestamp MineCommit actually writes, not by the
  name merely ending in ".snapshot".
- A shortcut named like a copy, pointing at a real world, would have had the
  world at the far end erased rather than the shortcut itself.
- Nothing now moves or erases a folder MineCommit is looking after as a world,
  whatever its name looks like, nor one that holds no world at all.
- Deleting the backups for a world checks that what it is about to erase really
  is a backup repository. The path comes from a settings file, and a settings
  file is something on disk that anything could have written.
- Downloading a world refuses to start if the folder it would keep its backups
  in already exists, because a world called "myworld.git" sits exactly there and
  the cleanup after a failed download would have removed it.
- A restore that cannot put the new world in place, and cannot move the original
  back either, now names the folder the original is in and where to move it. It
  said neither before, which is indistinguishable from having destroyed it.

## 0.11.1 (2026-08-23)

Bug fixes

- Worlds were disappearing from Minecraft's own world list while MineCommit sat
  idle, and coming back only when the game was restarted. The status check asked
  whether a world was in use by acquiring its session.lock, and while MineCommit
  holds that lock Minecraft cannot take it -- so Minecraft dropped the world from
  the list it builds for the game. Since 0.10.0 that check ran on a timer for
  every world for as long as the app was open, which turned a lock held for an
  instant into a collision that could recur all evening. It now asks whether the
  lock would conflict instead of taking it.

  The world you were playing was never at risk: the check fails when Minecraft
  holds the lock, so it could not take one out from under a running game. Only
  closed worlds were briefly locked, and only the list was affected. Nothing on
  disk was changed.

  On Windows there is no way to test a lock without taking it, so the badge no
  longer says whether Minecraft has a world open. Backing up is unaffected: it
  still checks properly, and still refuses to run on a world the game has open.

## 0.11.0 (2026-08-23)

Going back to an earlier backup keeps the world it replaced, and until now it
kept it right beside the original. Minecraft lists every folder in the saves
directory that holds a level.dat, so each of those copies turned up in the game
as a world of its own, with names differing only by a Unix timestamp. Choosing
what to play became a guessing game.

Fixes

- Copies are now kept next to the backup repositories, outside the saves folder,
  so Minecraft never sees them. If the saves folder is on a different drive to
  the repositories, where moving between them is not possible, the copy stays
  beside the world as before rather than the restore failing over where its
  safety copy goes.
- The copies already among your worlds can be cleared from the app. A banner
  counts them, and a dialog lists each one with the world it came from, when it
  was set aside, and what it takes up. Moving them out is the offered action and
  deleting is the second one: a copy is the world as it was before a restore, so
  it can hold play time that was never backed up anywhere else.

## 0.10.0 (2026-08-23)

New

- A backup now shows how far it has got: a progress bar with a percentage, the
  number of files done out of the total, how long it has been running, and an
  estimate of how much longer. The estimate is held back until the run is a
  twentieth of the way in, because before that it is measured over so little
  work that it swings by minutes between updates. A restore has no total to
  divide by, so it counts files rather than guessing at a percentage.
- Worlds are re-checked against GitHub on their own, every three minutes, so
  the list stays true after a session of play instead of showing what was true
  when the app opened. The check is skipped while a backup is running and while
  the window is hidden, and goes one world at a time rather than firing a burst
  of requests. There is a button in the top right for anyone who does not want
  to wait for the timer, and the world panel says when its answer was taken.

## 0.9.1 (2026-08-22)

Bug fixes

- The GitHub button in the top right did nothing when you were signed in. The
  menu behind it was never reachable, which meant signing out, opening your
  profile, and changing which repositories MineCommit may use were all
  unavailable once you had an account. The menu is now written out directly
  rather than built on a dropdown component whose trigger never opened it.

## 0.9.0 (2026-08-22)

The app now explains itself. It was built for someone who already knew what it
did; this release is for a Minecraft player who has never used Git and may not
have a GitHub account.

New

- A welcome screen on first run, saying what MineCommit promises rather than
  asking for a world it has not yet earned.
- A guide behind the header, answering what decides whether someone trusts the
  app with a world they care about: that backups never overwrite, that closing
  Minecraft comes first, what happens when two computers disagree, that a
  restore keeps the world it replaced, why GitHub, and what it can reach.
- A walkthrough for setting up online backup. Two of its steps happen on
  github.com and MineCommit cannot do either for you -- creating a repository
  or granting access to one both need authority over your whole account, which
  is exactly the breadth that picking individual repositories avoids. So it
  names the buttons in GitHub's own words, and watches for the permission
  rather than asking you to come back and press refresh.
- Downloading a world from GitHub no longer asks for a URL. You are signed in
  and the spaces you granted are already known, so it is two dropdowns. Pasting
  a link is still there for a world someone else shared with you.

Loading

- The account chip said "Sign in to GitHub" to already signed-in players until
  the saved account finished loading. World badges, the last-backup line and
  the history all appeared out of nothing. Each now has a placeholder of the
  right shape.
- A backup counts its own elapsed time and says outright, past twenty seconds,
  that the first backup of a world takes minutes and is safe to leave running.

Bug fixes

- The copies a restore leaves behind were offered as worlds to back up. They
  hold a level.dat like any real world, and two entries differing only by a
  Unix timestamp are not something you can tell apart.
- A scan of the saves folder that ran before the folder finished loading left a
  red error on screen for the rest of the dialog's life, underneath the list
  that had since loaded correctly.
- The repository dropdown rendered blank whenever its stored selection did not
  match the loaded list, above a button that did nothing and never said why.
- Signed in with no repositories granted was an empty dropdown and no way
  forward.
- A footer label overflowed its dialog. The interface is entirely monospace, so
  English runs about 1.6x wider than it looks in the source.
- The note box sat below "Back up now", which is where a note is one nobody
  writes, and Restore was invisible until you hovered the row it belonged to.
- The saves folder path filled the header on a Prism or MultiMC install.

## 0.8.2 (2026-08-22)

Bug fixes

- Say that repacking is still running. Git only draws progress when its stderr
  is a terminal, and MineCommit pipes it, so compressing a large world printed
  nothing at all for however many minutes it took, which is indistinguishable
  from a freeze. It now reports every ten seconds, with how much of the new
  pack has been written.
- Stop repacking after every backup. The repack recomputes every delta from
  scratch, which is what makes a world small, but redoing it because an evening
  of play added a few hundred objects to a pack holding hundreds of thousands
  is minutes spent for almost nothing. It now runs on the first backup and once
  loose objects pass Git's own threshold; otherwise it is skipped, which costs
  nothing because Git and the upload both handle loose objects.

Note on large worlds

- The first backup of a multi-gigabyte world writes hundreds of thousands of
  small files and then reads them all back. Measured on Linux, repacking scales
  linearly at roughly eight thousand objects a second, so even a million-object
  world takes a couple of minutes. On Windows the same work can take far longer
  because real-time virus scanning inspects every one of those files;
  excluding the minecommit folder from Windows Defender avoids it.

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
