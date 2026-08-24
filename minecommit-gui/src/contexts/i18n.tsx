import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react"

export type Locale = "en" | "zh-CN"

const LOCALE_STORAGE_KEY = "minecommit.locale"

export const localeOptions: Array<{ value: Locale; label: string }> = [
  { value: "en", label: "English" },
  { value: "zh-CN", label: "中文（简体）" },
]

/**
 * Every word the app says.
 *
 * The reader is a Minecraft player who has never heard of Git and may not have
 * a GitHub account. So: no "commit", no "push", no "remote", no "branch". A
 * backup is a backup. GitHub is named only where the player has to go there,
 * and where they do, the buttons are quoted exactly as GitHub labels them --
 * guessing which control is meant is where people give up.
 */
const en = {
  "common.cancel": "Cancel",
  "common.close": "Close",
  "common.back": "Back",
  "common.checkNow": "Check now",

  /* ── Log window ─────────────────────────────────────────────────────── */

  "logs.commit": "Backup",
  "logs.restore": "Restore",
  "logs.push": "Upload",
  "logs.pull": "Download",
  "logs.title": "{operation} log",
  "logs.empty": "No logs yet",
  "logs.forceStop": "Force stop",

  /* ── Failures worth explaining ──────────────────────────────────────── */

  "cloud.gitMissing":
    "Git is missing from this computer. Install it, then reopen MineCommit — you will never have to run a Git command yourself.",
  "cloud.authenticationFailed":
    "GitHub would not accept the sign-in. Sign out and back in from the menu in the top right.",
  "cloud.networkUnavailable":
    "Could not reach GitHub. Check your internet connection and try again.",
  "cloud.cannotCheck": "Could not check GitHub",

  /* ── Dashboard chrome ───────────────────────────────────────────────── */

  "dash.loading": "Loading your worlds…",
  "dash.worlds": "Worlds",
  "dash.addWorld": "Add world",
  "dash.noWorlds": "No worlds yet",
  "dash.selectWorld": "Pick a world on the left",
  "dash.savesFolder": "Saves folder",
  "dash.savesFolderHelp":
    "The folder Minecraft keeps your worlds in. MineCommit looks here for worlds to back up.",
  "dash.change": "Change",
  "dash.settings": "Settings",
  "dash.help": "How this works",
  "dash.history": "Backups",
  "dash.noHistory": "No backups yet. The first one you make will show up here.",
  "dash.restore": "Restore",
  "dash.lastBackedUp": "Last backed up {when}",
  "dash.neverBackedUp": "Never backed up",
  "dash.addNote": "Add a note",
  "dash.notePlaceholder": "What did you build? (optional)",
  "dash.thisDevice": "this PC",
  "dash.unknownDevice": "another computer",
  "dash.remove": "Remove world",
  "dash.showLog": "Show details",
  "dash.checking": "Checking…",
  "dash.recheck": "Check again",
  "dash.checkNow": "Check every world now",
  "dash.checkedAgo": "checked {when}",

  /* ── First run ──────────────────────────────────────────────────────── */

  "welcome.title": "Never lose a world again",
  "welcome.body":
    "MineCommit keeps a complete copy of your Minecraft worlds every time you ask it to. A creeper in the wrong place, a save that will not load, a hard drive that dies — none of them have to be the end of the world.",
  "welcome.safe.title": "Nothing is ever overwritten",
  "welcome.safe.body": "Every backup is kept, so you can go back to any day you like.",
  "welcome.anywhere.title": "Play on any computer",
  "welcome.anywhere.body":
    "Back up on one PC and carry on where you left off on another.",
  "welcome.private.title": "Yours and nobody else's",
  "welcome.private.body":
    "Worlds are stored in a private space on GitHub that only you can open.",
  "welcome.start": "Choose a world to back up",
  "welcome.how": "How does this work?",

  /* ── World list badges ──────────────────────────────────────────────── */

  "badge.checking": "Checking…",
  "badge.backedUp": "Backed up",
  "badge.needsBackup": "Not backed up",
  "badge.newerInCloud": "Newer copy online",
  "badge.conflict": "Two versions",
  "badge.localOnly": "This PC only",
  "badge.inUse": "Minecraft is open",

  /* ── What to do next ────────────────────────────────────────────────── */

  "state.noCloud.title": "This world only exists on this PC",
  "state.noCloud.body":
    "It is being backed up here, which survives a bad save — but not a dead drive. Put it online and you can also play it on another computer.",
  "state.noCloud.action": "Set up online backup",
  "state.firstBackup.title": "Ready for its first backup",
  "state.firstBackup.body":
    "Nothing has been saved yet. The first backup copies the whole world, so give it a few minutes.",
  "state.needsBackup.title": "You have played since the last backup",
  "state.needsBackup.body":
    "Whatever you built since then is not saved anywhere yet.",
  "state.upToDate.title": "Everything is backed up",
  "state.upToDate.body": "This world matches the copy stored online.",
  "state.newerInCloud.title": "A newer copy is waiting online",
  "state.newerInCloud.body":
    "Another computer backed this world up after you did. Getting it replaces the world on this PC — a dated copy of what is here now is kept beside it, so nothing is thrown away.",
  "state.newerInCloud.action": "Get the latest world",
  "state.conflict.title": "This world was played on two computers",
  "state.conflict.body":
    "Both copies have backups the other one has not. Getting the latest keeps a dated copy of this PC's world next to it, so you can still go back to either.",
  "state.inUse.title": "Close Minecraft first",
  "state.inUse.body":
    "A world is only finished being written when you leave to the title screen, so MineCommit will not copy it while the game still has it open.",
  "state.backUpNow": "Back up now",
  "state.backingUp": "Backing up…",
  "state.backingUpSlow":
    "The first backup of a world copies all of it, which can take several minutes. It is safe to leave this running.",
  "state.gettingLatest": "Getting the latest world…",
  "state.phase.reading": "Reading your world…",
  "state.phase.writing": "Putting the world back together…",
  "state.phase.downloading": "Downloading from GitHub…",
  "state.phase.uploading": "Uploading to GitHub…",
  "state.elapsed": "{time} so far",
  "state.filesDone": "{done} of {total} files",
  "state.filesWritten": "{done} files so far",
  "state.downloaded": "{size} so far",
  "state.secondsLeft": "about {seconds} seconds left",
  "state.minutesLeft": "about {minutes} min left",
  "state.hoursLeft": "about {hours} hours left",

  /* ── What a world is ────────────────────────────────────────────────── */

  "world.details": "About this world",
  "world.detailsUnreadable":
    "This world does not say which version it is from. That changes nothing about backing it up.",
  "world.version": "Version",
  "world.snapshotVersion": "{version} (snapshot)",
  "world.mode": "Mode",
  "world.difficulty": "Difficulty",
  "world.difficultyLocked": "{difficulty}, locked",
  "world.size": "Size on disk",
  "world.age": "In-game day",
  "world.day": "Day {day}",
  "world.seed": "Seed",
  "world.copySeed": "Copy",
  "world.copied": "Copied",
  "world.spawn": "Spawn",
  "world.dataPacks": "Data packs",
  "world.inGameName": "In Minecraft this world is called “{name}”",
  "world.mode.survival": "Survival",
  "world.mode.creative": "Creative",
  "world.mode.adventure": "Adventure",
  "world.mode.spectator": "Spectator",
  "world.difficulty.peaceful": "Peaceful",
  "world.difficulty.easy": "Easy",
  "world.difficulty.normal": "Normal",
  "world.difficulty.hard": "Hard",
  "world.hardcore": "Hardcore",
  "world.cheats": "Cheats on",
  "world.modded": "Modded",

  /* ── How it went ────────────────────────────────────────────────────── */

  "result.done": "Backed up and uploaded",
  "result.localOnly": "Backed up on this PC",
  "result.uploadFailed": "Backed up here, but it could not be uploaded",
  "result.uploadFailedHelp":
    "Your world is safe on this PC. The next backup will send it up along with everything else.",
  "result.failed": "The backup did not finish",
  "result.restored": "This PC now has the latest world",
  "result.retryUpload": "Try uploading again",

  /* ── Adding a world ─────────────────────────────────────────────────── */

  "add.title": "Add a world",
  "add.fromThisPc": "On this PC",
  "add.fromCloud": "From GitHub",
  "add.scanning": "Looking through your saves folder…",
  "add.noneFound":
    "No Minecraft worlds in this folder. If your worlds live somewhere else, change the saves folder in Settings.",
  "add.allAdded": "Every world in this folder has already been added.",
  "add.played": "played {when}",
  "add.neverPlayed": "never played",
  "add.add": "Add",
  "add.adding": "Adding…",
  "add.cloudIntro":
    "Worlds you backed up from another computer can be brought down here.",
  "add.cloudSignIn":
    "Sign in to GitHub to see the worlds you have already backed up.",
  "add.cloudRepo": "Backup space",
  "add.cloudNoSpaces":
    "MineCommit cannot see any backup spaces yet. Choose which ones it may use on GitHub, or paste a link if someone shared one with you.",
  "add.cloudLoadingWorlds": "Looking for worlds…",
  "add.cloudNoWorlds":
    "Nothing has been backed up to this space yet. Back a world up from your other computer first.",
  "add.cloudWorld": "World",
  "add.cloudName": "Save it as",
  "add.cloudNameHelp": "It will appear in Minecraft under this name.",
  "add.download": "Download",
  "add.downloading": "Downloading…",
  "add.downloadingHelp": "A large world can take several minutes.",
  "add.haveLink": "I was given a link instead",
  "add.usePicker": "Pick from my backup spaces",
  "add.cloudAddress": "Link",
  "add.cloudAddressHelp":
    "The GitHub address someone shared with you, for example https://github.com/them/our-world.git",
  "add.cloudLookup": "Look up",
  "add.cloudLookingUp": "Looking up…",

  /* ── Setting up online backup ───────────────────────────────────────── */

  "setup.title": "Back up “{world}” online",
  "setup.stepOf": "Step {n} of {total}",

  "setup.signIn.title": "First, sign in to GitHub",
  "setup.signIn.body":
    "GitHub is a free service for storing files online. MineCommit uses it as the safe place your worlds are kept. You only ever do this once.",
  "setup.signIn.free":
    "Free, with far more room than your worlds will ever need.",
  "setup.signIn.private":
    "Private. Your worlds are not published and nobody else can open them.",
  "setup.signIn.password":
    "MineCommit never sees your password — you sign in on GitHub's own website.",
  "setup.signIn.noAccount": "No account yet? Making one is free and takes a minute.",
  "setup.signIn.createAccount": "Create a GitHub account",

  "setup.create.title": "Make a place for your worlds",
  "setup.create.body":
    "Your worlds need a private space on GitHub to live in. GitHub calls this a repository; think of it as a folder that remembers every version of what you put in it. One space can hold all of your worlds, so you only do this once.",
  "setup.create.step1":
    "Press the button below. GitHub opens with the name “{name}” already filled in — change it if you like.",
  "setup.create.step2": "Leave it set to Private.",
  "setup.create.step3":
    "Do not tick any of the boxes underneath — no README, no .gitignore, no licence. The space has to start out empty.",
  "setup.create.step4": "Press the green Create repository button at the bottom.",
  "setup.create.open": "Open GitHub and make it",
  "setup.create.made": "I have made it",
  "setup.create.skip": "Already have one? Skip this step.",

  "setup.grant.title": "Let MineCommit use it",
  "setup.grant.body":
    "GitHub asks you which spaces MineCommit is allowed to open. Tick the one you just made — everything else in your account stays out of reach, now and later.",
  "setup.grant.step1": "Press the button below.",
  "setup.grant.step2": "Choose Only select repositories.",
  "setup.grant.step3": "Tick the space you just made, then press Install.",
  "setup.grant.open": "Choose it on GitHub",
  "setup.grant.waiting":
    "Watching for it — this moves on by itself as soon as GitHub is done.",
  "setup.grant.stopped":
    "Stopped watching. Press Check now once you have finished on GitHub.",
  "setup.grant.found": "Found it.",

  "setup.pick.title": "Where should “{world}” be kept?",
  "setup.pick.body":
    "One space can hold several worlds. Each keeps its own name and they never mix.",
  "setup.pick.repo": "Backup space",
  "setup.pick.repoHelp": "Only spaces you ticked on GitHub appear here.",
  "setup.pick.name": "Name for this world",
  "setup.pick.nameHelp":
    "Any computer that downloads this world will see it under this name.",
  "setup.pick.action": "Back up here from now on",
  "setup.pick.working": "Connecting…",
  "setup.pick.another": "Use a different space",
  "setup.pick.loading": "Looking for your backup spaces…",
  "setup.pick.private": "private",

  /* ── Signing in ─────────────────────────────────────────────────────── */

  "gh.account": "GitHub",
  "gh.signIn": "Sign in to GitHub",
  "gh.signedInAs": "Signed in as {login}",
  "gh.signOut": "Sign out",
  "gh.openProfile": "Open GitHub profile",
  "gh.chooseRepos": "Choose backup spaces",
  "gh.signInTitle": "Sign in to GitHub",
  "gh.signInBody":
    "GitHub will show you a short code to type on its own website. MineCommit never sees your password.",
  "gh.howItWorks":
    "Pressing the button opens GitHub in your browser and shows you a code. Type the code there, approve MineCommit, and this window will notice by itself.",
  "gh.enterCode": "Type this code on the GitHub page that just opened:",
  "gh.tapToCopy": "Click the code to copy it",
  "gh.copy": "Copy",
  "gh.copied": "Copied",
  "gh.waiting": "Waiting for you to approve it on GitHub…",
  "gh.signingIn": "Starting…",
  "gh.reopenPage": "Open the GitHub page again",
  "gh.noAccount": "No GitHub account yet?",
  "gh.createAccount": "Create one, it is free",
  "gh.denied": "The sign-in was turned down on GitHub. Nothing has changed.",
  "gh.expired": "That code ran out of time. Start again to get a fresh one.",

  /* ── Restore, remove, settings ──────────────────────────────────────── */

  "restoreTo.title": "Go back to this backup?",
  "restoreTo.body":
    "The world on this PC will be put back to how it was on {when}. What is there right now is kept beside it as a dated copy, so you can change your mind.",
  "restoreTo.action": "Go back",
  "restoreTo.working": "Putting the world back…",

  "removeWorld.title": "Remove “{world}”?",
  "removeWorld.body":
    "MineCommit stops looking after this world. The world itself stays in your saves folder and Minecraft can still play it.",
  "removeWorld.alsoBackups": "Also delete the backups kept on this PC",
  "removeWorld.action": "Remove",
  "removeWorld.removing": "Removing…",

  "settings.language": "Language",
  "settings.name": "Your name",
  "settings.namePlaceholder": "For example: Steve",
  "settings.email": "Email",
  "settings.emailPlaceholder": "For example: steve@example.com",
  "settings.identityHelp":
    "Written next to each backup so you can tell which of your computers made it. It is not shown to anyone else.",

  /* ── The guide ──────────────────────────────────────────────────────── */

  "oldCopies.banner": "{count} old world copies are showing up in Minecraft",
  "oldCopies.review": "Sort this out",
  "oldCopies.title": "Copies left among your worlds",
  "oldCopies.body":
    "Every time you went back to an earlier backup, MineCommit kept the world you had at that moment. They were left in your saves folder, which is why Minecraft lists them next to your real worlds. Moving them out gets them out of the game and keeps every one of them.",
  "oldCopies.scanning": "Adding up what they take…",
  "oldCopies.none": "Nothing left to tidy up.",
  "oldCopies.from": "from {world}",
  "oldCopies.taken": "kept {when}",
  "oldCopies.total": "{size} in total",
  "oldCopies.moveAction": "Move them out of Minecraft",
  "oldCopies.moving": "Moving…",
  "oldCopies.moved":
    "Moved out of your saves folder. Minecraft will not list them any more, and they are still there if you need them: {where}",
  "oldCopies.deleteAction": "Delete instead",
  "oldCopies.deleting": "Deleting…",
  "oldCopies.deleteConfirm":
    "Delete these for good? A copy can hold play time that was never backed up anywhere else.",
  "oldCopies.deleteYes": "Delete them for good",

  "guide.title": "How MineCommit works",

  "guide.what.topic": "What this does",
  "guide.what.p1":
    "Every time you press Back up now, MineCommit takes a complete copy of the world and keeps it. The copy is not put anywhere near your save folder, so nothing you do in the game can damage it.",
  "guide.what.p2":
    "Backing up again does not replace the last copy — it adds another one to the list. That list is the point of the whole app: any backup in it can be put back later.",
  "guide.what.p3":
    "Only the parts that changed take up room. A world you have backed up fifty times does not take fifty times the space, so back up as often as you like.",
  "guide.what.p4":
    "MineCommit does not touch Minecraft. No mods, no plugins, no launcher settings. Your worlds stay ordinary worlds.",

  "guide.backup.topic": "Backing up",
  "guide.backup.p1":
    "Close Minecraft first, or at least leave to the title screen. A world is still being written while you are in it, and half a world is not worth saving.",
  "guide.backup.p2":
    "Press Back up now. MineCommit copies the world, then sends it to GitHub if you have set that up. Both happen from the one button.",
  "guide.backup.p3":
    "The first backup is the slow one — a big world can take several minutes. After that only what changed gets sent, so later backups usually take seconds.",
  "guide.backup.p4":
    "Notes are worth writing. “Finished the mob farm” tells you far more six months later than a date does, and the note is what you scan the list for.",

  "guide.twoPcs.topic": "Two computers",
  "guide.twoPcs.p1":
    "Back up when you stop playing. Get the latest world when you start. That is the whole routine.",
  "guide.twoPcs.p2":
    "On the second computer, add the world with Add world → From GitHub. It downloads into your saves folder and Minecraft picks it up like any other world.",
  "guide.twoPcs.p3":
    "Forget the routine and play on both, and MineCommit will tell you the world was played on two computers. Nothing is lost: it keeps both versions and lets you choose which one carries on.",
  "guide.twoPcs.p4":
    "Worlds are never merged together. Two people mining in the same chunk cannot be stitched into one world, and pretending otherwise would quietly ruin builds.",

  "guide.history.topic": "Going back",
  "guide.history.p1":
    "Every backup in the list has a Restore button. Choosing one puts the world back exactly as it was at that moment.",
  "guide.history.p2":
    "The world you had before restoring is not deleted. It is renamed with the date and left next to your other worlds, so a restore can itself be undone.",
  "guide.history.p3":
    "Those dated copies stay until you remove them. If your saves folder starts looking crowded, the folders ending in .snapshot are the ones you can safely delete.",

  "guide.github.topic": "Why GitHub?",
  "guide.github.p1":
    "MineCommit has no servers. Nobody at MineCommit stores your worlds, sees them, or pays a hosting bill that could stop being paid one day.",
  "guide.github.p2":
    "GitHub gives everyone free private storage and is built for exactly this job: keeping every version of a set of files and sending only what changed. Your worlds go into a private space that only your account can open.",
  "guide.github.p3":
    "MineCommit can only reach the spaces you tick when you set it up. It cannot list the rest of your account, and cannot create anything on your behalf.",
  "guide.github.p4":
    "Your password never passes through MineCommit. You sign in on github.com, and the permission it hands back is kept by the same part of Windows, macOS or Linux that stores your other sign-ins.",

  "guide.where.topic": "Where they live",
  "guide.where.p1":
    "On this PC: in a minecommit folder beside your saves. This copy works with no internet at all, and is what a restore reads from.",
  "guide.where.p2":
    "Online: in the GitHub space you connected the world to, once you have connected one.",
  "guide.where.p3":
    "A world with no GitHub connection is still backed up — but only here. If the drive fails, the world and its backups go together, which is the one thing setting up GitHub protects you from.",

  "guide.trouble.topic": "If it goes wrong",
  "guide.trouble.p1":
    "“Close Minecraft first” means the game still has the world open. Quit to the title screen, or close Minecraft, then try again.",
  "guide.trouble.p2":
    "If the upload fails, your world is still backed up on this PC. Check your internet and press Back up now again — the next one sends everything that is waiting.",
  "guide.trouble.p3":
    "On Windows, backups that crawl are usually Windows Security scanning every file MineCommit writes. Adding the minecommit folder to its exclusion list makes a large world far faster.",
  "guide.trouble.p4":
    "Show details opens the full log of whatever just ran. If you report a problem, that log is the useful thing to send.",
} as const

export type TranslationKey = keyof typeof en

const zhCN: Record<TranslationKey, string> = {
  "common.cancel": "取消",
  "common.close": "关闭",
  "common.back": "返回",
  "common.checkNow": "立即检查",

  "logs.commit": "备份",
  "logs.restore": "还原",
  "logs.push": "上传",
  "logs.pull": "下载",
  "logs.title": "{operation}日志",
  "logs.empty": "暂无日志",
  "logs.forceStop": "强制停止",

  "cloud.gitMissing":
    "这台电脑上没有 Git。安装后重新打开 MineCommit —— 你永远不需要自己敲 Git 命令。",
  "cloud.authenticationFailed":
    "GitHub 没有接受这次登录。请从右上角的菜单退出登录后重新登录。",
  "cloud.networkUnavailable": "连不上 GitHub。请检查网络后重试。",
  "cloud.cannotCheck": "无法检查 GitHub",

  "dash.loading": "正在加载你的存档…",
  "dash.worlds": "存档",
  "dash.addWorld": "添加存档",
  "dash.noWorlds": "还没有存档",
  "dash.selectWorld": "在左侧选择一个存档",
  "dash.savesFolder": "存档文件夹",
  "dash.savesFolderHelp":
    "Minecraft 存放世界的文件夹。MineCommit 会在这里寻找可以备份的存档。",
  "dash.change": "更改",
  "dash.settings": "设置",
  "dash.help": "使用说明",
  "dash.history": "备份记录",
  "dash.noHistory": "还没有备份。你做的第一个备份会出现在这里。",
  "dash.restore": "还原",
  "dash.lastBackedUp": "上次备份于{when}",
  "dash.neverBackedUp": "从未备份",
  "dash.addNote": "写点备注",
  "dash.notePlaceholder": "这次造了什么？（可不填）",
  "dash.thisDevice": "这台电脑",
  "dash.unknownDevice": "另一台电脑",
  "dash.remove": "移除存档",
  "dash.showLog": "查看详情",
  "dash.checking": "检查中…",
  "dash.recheck": "再检查一次",
  "dash.checkNow": "立即检查所有存档",
  "dash.checkedAgo": "{when}检查过",

  "welcome.title": "再也不会弄丢世界",
  "welcome.body":
    "每次你需要时，MineCommit 都会为存档保存一份完整副本。苦力怕炸错了地方、存档打不开、硬盘坏了 —— 都不必是世界的终点。",
  "welcome.safe.title": "永远不会被覆盖",
  "welcome.safe.body": "每一次备份都会保留，随时可以回到任何一天。",
  "welcome.anywhere.title": "换电脑照样玩",
  "welcome.anywhere.body": "在一台电脑上备份，在另一台上接着玩。",
  "welcome.private.title": "只属于你",
  "welcome.private.body": "存档保存在 GitHub 上只有你能打开的私有空间里。",
  "welcome.start": "选择要备份的存档",
  "welcome.how": "它是怎么工作的？",

  "badge.checking": "检查中…",
  "badge.backedUp": "已备份",
  "badge.needsBackup": "未备份",
  "badge.newerInCloud": "云端有更新",
  "badge.conflict": "有两个版本",
  "badge.localOnly": "仅本机",
  "badge.inUse": "Minecraft 开着",

  "state.noCloud.title": "这个存档只存在于本机",
  "state.noCloud.body":
    "它已经在本机备份了，存档损坏不怕，但硬盘坏了就都没了。传到线上还能在别的电脑上继续玩。",
  "state.noCloud.action": "设置线上备份",
  "state.firstBackup.title": "可以做第一次备份了",
  "state.firstBackup.body":
    "还没有保存过任何内容。第一次备份会复制整个世界，请给它几分钟。",
  "state.needsBackup.title": "上次备份之后你又玩过了",
  "state.needsBackup.body": "这之后造的东西还没有保存到任何地方。",
  "state.upToDate.title": "全部已备份",
  "state.upToDate.body": "这个存档和线上的副本一致。",
  "state.newerInCloud.title": "线上有更新的副本",
  "state.newerInCloud.body":
    "另一台电脑在你之后备份过这个存档。取回它会替换本机的世界 —— 现在的内容会以带日期的副本留在旁边，不会丢。",
  "state.newerInCloud.action": "取回最新的世界",
  "state.conflict.title": "这个存档在两台电脑上都玩过",
  "state.conflict.body":
    "两边都有对方没有的备份。取回最新版本时，本机现在的世界会以带日期的副本保留，两个版本都还能回去。",
  "state.inUse.title": "请先关闭 Minecraft",
  "state.inUse.body":
    "只有回到标题界面，世界才算写完。游戏还开着时 MineCommit 不会去复制它。",
  "state.backUpNow": "立即备份",
  "state.backingUp": "备份中…",
  "state.backingUpSlow":
    "一个存档的第一次备份要复制全部内容，可能需要几分钟。让它跑着就好。",
  "state.gettingLatest": "正在取回最新的世界…",
  "state.phase.reading": "正在读取你的世界…",
  "state.phase.writing": "正在重新拼回世界…",
  "state.phase.downloading": "正在从 GitHub 下载…",
  "state.phase.uploading": "正在上传到 GitHub…",
  "state.elapsed": "已用 {time}",
  "state.filesDone": "{total} 个文件中的第 {done} 个",
  "state.filesWritten": "已处理 {done} 个文件",
  "state.downloaded": "已传 {size}",
  "state.secondsLeft": "大约还剩 {seconds} 秒",
  "state.minutesLeft": "大约还剩 {minutes} 分钟",
  "state.hoursLeft": "大约还剩 {hours} 小时",

  "world.details": "关于这个世界",
  "world.detailsUnreadable": "这个世界没有写明自己来自哪个版本。这不影响备份。",
  "world.version": "版本",
  "world.snapshotVersion": "{version}（快照版）",
  "world.mode": "模式",
  "world.difficulty": "难度",
  "world.difficultyLocked": "{difficulty}，已锁定",
  "world.size": "占用空间",
  "world.age": "游戏内天数",
  "world.day": "第 {day} 天",
  "world.seed": "种子",
  "world.copySeed": "复制",
  "world.copied": "已复制",
  "world.spawn": "出生点",
  "world.dataPacks": "数据包",
  "world.inGameName": "在 Minecraft 里这个世界叫“{name}”",
  "world.mode.survival": "生存",
  "world.mode.creative": "创造",
  "world.mode.adventure": "冒险",
  "world.mode.spectator": "旁观",
  "world.difficulty.peaceful": "和平",
  "world.difficulty.easy": "简单",
  "world.difficulty.normal": "普通",
  "world.difficulty.hard": "困难",
  "world.hardcore": "极限模式",
  "world.cheats": "已开作弊",
  "world.modded": "已装模组",

  "result.done": "已备份并上传",
  "result.localOnly": "已在本机备份",
  "result.uploadFailed": "已在本机备份，但没能上传",
  "result.uploadFailedHelp":
    "你的世界在本机是安全的。下次备份会把它和其他内容一起传上去。",
  "result.failed": "备份没有完成",
  "result.restored": "本机已经是最新的世界了",
  "result.retryUpload": "重新上传",

  "add.title": "添加存档",
  "add.fromThisPc": "本机上的",
  "add.fromCloud": "从 GitHub",
  "add.scanning": "正在查看你的存档文件夹…",
  "add.noneFound":
    "这个文件夹里没有 Minecraft 存档。如果你的存档在别处，请到设置里更改存档文件夹。",
  "add.allAdded": "这个文件夹里的存档都已经添加过了。",
  "add.played": "{when}玩过",
  "add.neverPlayed": "从未玩过",
  "add.add": "添加",
  "add.adding": "添加中…",
  "add.cloudIntro": "在另一台电脑上备份过的存档，可以在这里取下来。",
  "add.cloudSignIn": "登录 GitHub 就能看到你已经备份过的存档。",
  "add.cloudRepo": "备份空间",
  "add.cloudNoSpaces":
    "MineCommit 还看不到任何备份空间。去 GitHub 上选择它可以使用的空间，或者粘贴别人分享给你的链接。",
  "add.cloudLoadingWorlds": "正在查找存档…",
  "add.cloudNoWorlds":
    "这个空间里还没有备份过任何存档。请先在另一台电脑上备份一次。",
  "add.cloudWorld": "存档",
  "add.cloudName": "保存为",
  "add.cloudNameHelp": "它会以这个名字出现在 Minecraft 里。",
  "add.download": "下载",
  "add.downloading": "下载中…",
  "add.downloadingHelp": "大的存档可能需要几分钟。",
  "add.haveLink": "别人给了我一个链接",
  "add.usePicker": "从我的备份空间里选",
  "add.cloudAddress": "链接",
  "add.cloudAddressHelp":
    "别人分享给你的 GitHub 地址，例如 https://github.com/them/our-world.git",
  "add.cloudLookup": "查找",
  "add.cloudLookingUp": "查找中…",

  "setup.title": "把“{world}”备份到线上",
  "setup.stepOf": "第 {n} 步，共 {total} 步",

  "setup.signIn.title": "第一步，登录 GitHub",
  "setup.signIn.body":
    "GitHub 是一个免费的在线文件存放服务。MineCommit 用它来安全地保管你的存档。这一步只需要做一次。",
  "setup.signIn.free": "免费，空间远超你的存档所需。",
  "setup.signIn.private": "私有。你的存档不会被公开，别人也打不开。",
  "setup.signIn.password":
    "MineCommit 看不到你的密码 —— 你是在 GitHub 自己的网站上登录的。",
  "setup.signIn.noAccount": "还没有账号？注册是免费的，一分钟就好。",
  "setup.signIn.createAccount": "注册 GitHub 账号",

  "setup.create.title": "给你的存档准备一个地方",
  "setup.create.body":
    "你的存档需要一个 GitHub 上的私有空间来存放。GitHub 把它叫做仓库（repository），你可以把它当成一个会记住每个版本的文件夹。一个空间可以放下你所有的存档，所以这一步只做一次。",
  "setup.create.step1":
    "点下面的按钮。GitHub 会打开，名字“{name}”已经帮你填好了 —— 想改也可以。",
  "setup.create.step2": "保持选择 Private（私有）。",
  "setup.create.step3":
    "下面那几个勾一个都不要打 —— 不要 README、不要 .gitignore、不要 licence。这个空间必须是空的。",
  "setup.create.step4": "点最下面绿色的 Create repository 按钮。",
  "setup.create.open": "打开 GitHub 去创建",
  "setup.create.made": "我已经创建好了",
  "setup.create.skip": "已经有了？跳过这一步。",

  "setup.grant.title": "让 MineCommit 可以使用它",
  "setup.grant.body":
    "GitHub 会问你允许 MineCommit 打开哪些空间。勾上你刚创建的那个 —— 账号里的其他内容它永远碰不到。",
  "setup.grant.step1": "点下面的按钮。",
  "setup.grant.step2": "选择 Only select repositories（仅选定的仓库）。",
  "setup.grant.step3": "勾上你刚创建的那个空间，然后点 Install。",
  "setup.grant.open": "去 GitHub 选择",
  "setup.grant.waiting": "正在等待 —— GitHub 那边一完成，这里会自动继续。",
  "setup.grant.stopped": "已停止等待。在 GitHub 上弄好之后点“立即检查”。",
  "setup.grant.found": "找到了。",

  "setup.pick.title": "“{world}”存到哪里？",
  "setup.pick.body": "一个空间可以放好几个存档，各用各的名字，互不影响。",
  "setup.pick.repo": "备份空间",
  "setup.pick.repoHelp": "这里只会显示你在 GitHub 上勾选过的空间。",
  "setup.pick.name": "这个存档的名字",
  "setup.pick.nameHelp": "任何下载这个存档的电脑都会看到这个名字。",
  "setup.pick.action": "以后就备份到这里",
  "setup.pick.working": "连接中…",
  "setup.pick.another": "换一个空间",
  "setup.pick.loading": "正在查找你的备份空间…",
  "setup.pick.private": "私有",

  "gh.account": "GitHub",
  "gh.signIn": "登录 GitHub",
  "gh.signedInAs": "已登录：{login}",
  "gh.signOut": "退出登录",
  "gh.openProfile": "打开 GitHub 主页",
  "gh.chooseRepos": "选择备份空间",
  "gh.signInTitle": "登录 GitHub",
  "gh.signInBody":
    "GitHub 会给你一个短代码，在它自己的网站上输入即可。MineCommit 看不到你的密码。",
  "gh.howItWorks":
    "点击按钮会在浏览器里打开 GitHub 并显示一个代码。在那里输入代码并允许 MineCommit，这个窗口会自己发现。",
  "gh.enterCode": "在刚打开的 GitHub 页面里输入这个代码：",
  "gh.tapToCopy": "点击代码即可复制",
  "gh.copy": "复制",
  "gh.copied": "已复制",
  "gh.waiting": "等待你在 GitHub 上确认…",
  "gh.signingIn": "正在开始…",
  "gh.reopenPage": "重新打开 GitHub 页面",
  "gh.noAccount": "还没有 GitHub 账号？",
  "gh.createAccount": "免费注册一个",
  "gh.denied": "GitHub 上拒绝了这次登录。什么都没有改变。",
  "gh.expired": "代码已过期。重新开始就会给你一个新的。",

  "restoreTo.title": "回到这个备份？",
  "restoreTo.body":
    "本机的世界会被还原成{when}时的样子。现在的内容会以带日期的副本留在旁边，随时可以反悔。",
  "restoreTo.action": "回到这里",
  "restoreTo.working": "正在还原…",

  "removeWorld.title": "移除“{world}”？",
  "removeWorld.body":
    "MineCommit 不再管理这个存档。存档本身还留在你的存档文件夹里，Minecraft 照样能玩。",
  "removeWorld.alsoBackups": "同时删除保存在本机的备份",
  "removeWorld.action": "移除",
  "removeWorld.removing": "移除中…",

  "settings.language": "语言",
  "settings.name": "你的名字",
  "settings.namePlaceholder": "例如：Steve",
  "settings.email": "邮箱",
  "settings.emailPlaceholder": "例如：steve@example.com",
  "settings.identityHelp":
    "会写在每个备份旁边，方便你分辨是哪台电脑做的。不会展示给其他人。",

  "oldCopies.banner": "有 {count} 个旧存档副本出现在 Minecraft 里",
  "oldCopies.review": "去处理",
  "oldCopies.title": "混在存档里的旧副本",
  "oldCopies.body":
    "每次你回到更早的备份时，MineCommit 都会保留当时的那个世界。它们留在了存档文件夹里，所以 Minecraft 会把它们和你真正的存档并排列出来。把它们移出去就不会再出现在游戏里，而且一个都不会少。",
  "oldCopies.scanning": "正在统计它们占用的空间…",
  "oldCopies.none": "没有需要整理的了。",
  "oldCopies.from": "来自 {world}",
  "oldCopies.taken": "{when}保留",
  "oldCopies.total": "共 {size}",
  "oldCopies.moveAction": "把它们移出 Minecraft",
  "oldCopies.moving": "移动中…",
  "oldCopies.moved":
    "已移出存档文件夹。Minecraft 不会再列出它们，需要时仍然找得到：{where}",
  "oldCopies.deleteAction": "改为删除",
  "oldCopies.deleting": "删除中…",
  "oldCopies.deleteConfirm": "确定要永久删除吗？副本里可能有从未备份到别处的游戏进度。",
  "oldCopies.deleteYes": "永久删除",

  "guide.title": "MineCommit 是怎么工作的",

  "guide.what.topic": "它做什么",
  "guide.what.p1":
    "每次你按下“立即备份”，MineCommit 都会完整复制一份存档并保存起来。副本不会放在存档文件夹附近，所以游戏里做什么都伤不到它。",
  "guide.what.p2":
    "再次备份不会覆盖上一份，而是往列表里再加一份。这个列表就是整个应用的意义：里面任何一次备份都能还原回来。",
  "guide.what.p3":
    "只有变化的部分会占地方。备份五十次的存档不会占五十倍空间，所以想备份就备份。",
  "guide.what.p4":
    "MineCommit 不会改动 Minecraft。不需要模组、插件或启动器设置，你的存档还是普通存档。",

  "guide.backup.topic": "备份",
  "guide.backup.p1":
    "先关掉 Minecraft，至少回到标题界面。你还在世界里时它仍在写入，半个世界不值得保存。",
  "guide.backup.p2":
    "点“立即备份”。MineCommit 会复制存档，如果你设置过线上备份，还会把它传上去。一个按钮把两件事都做完。",
  "guide.backup.p3":
    "第一次备份最慢 —— 大的存档要几分钟。之后只传变化的部分，通常几秒钟就好。",
  "guide.backup.p4":
    "备注很值得写。半年后，“刷怪塔完工”比一个日期有用得多，你在列表里找的也正是这个。",

  "guide.twoPcs.topic": "两台电脑",
  "guide.twoPcs.p1": "不玩了就备份，开始玩之前取回最新的世界。整套流程就这两句。",
  "guide.twoPcs.p2":
    "在第二台电脑上，用“添加存档 → 从 GitHub”把存档加进来。它会下载到你的存档文件夹，Minecraft 会像识别普通存档一样识别它。",
  "guide.twoPcs.p3":
    "如果忘了这个顺序、两边都玩过，MineCommit 会告诉你这个存档在两台电脑上都玩过。什么都不会丢：两个版本都保留，由你决定接着用哪个。",
  "guide.twoPcs.p4":
    "存档永远不会被合并。两个人在同一个区块里挖矿，是没法缝成一个世界的，硬来只会悄悄毁掉建筑。",

  "guide.history.topic": "回到过去",
  "guide.history.p1":
    "列表里的每个备份都有“还原”按钮。选一个，世界就会变回那一刻的样子。",
  "guide.history.p2":
    "还原之前的世界不会被删除。它会被改成带日期的名字留在其他存档旁边，所以还原本身也可以反悔。",
  "guide.history.p3":
    "这些带日期的副本会一直留着，直到你自己删掉。如果存档文件夹显得拥挤，以 .snapshot 结尾的文件夹就是可以放心删掉的。",

  "guide.github.topic": "为什么是 GitHub",
  "guide.github.p1":
    "MineCommit 没有服务器。没有人替你保存存档、看你的存档，也没有哪天可能停付的托管账单。",
  "guide.github.p2":
    "GitHub 给每个人免费的私有存储，而且它本来就是干这个的：保留一组文件的每个版本，并且只传输变化的部分。你的存档会放进只有你的账号能打开的私有空间。",
  "guide.github.p3":
    "MineCommit 只能访问你在设置时勾选的空间。它列不出你账号里的其他内容，也不能替你创建任何东西。",
  "guide.github.p4":
    "你的密码不会经过 MineCommit。你在 github.com 上登录，之后拿到的凭据由 Windows、macOS 或 Linux 保管其他登录信息的同一个地方保存。",

  "guide.where.topic": "备份存在哪",
  "guide.where.p1":
    "本机：存档文件夹旁边的 minecommit 文件夹里。这份副本完全不需要网络，还原时读的就是它。",
  "guide.where.p2": "线上：如果你为存档设置过线上备份，就在你连接的那个 GitHub 空间里。",
  "guide.where.p3":
    "没有连接 GitHub 的存档也会被备份 —— 但只在本机。硬盘一坏，存档和备份一起没了，而这正是设置 GitHub 能挡住的事。",

  "guide.trouble.topic": "出问题的时候",
  "guide.trouble.p1":
    "“请先关闭 Minecraft”意味着游戏还开着这个世界。退回标题界面或关掉游戏，然后再试。",
  "guide.trouble.p2":
    "上传失败时，你的世界在本机仍然是备份好的。检查网络后再点一次“立即备份”—— 下一次会把积压的内容一起传上去。",
  "guide.trouble.p3":
    "在 Windows 上，备份特别慢通常是因为 Windows 安全中心在扫描 MineCommit 写入的每个文件。把 minecommit 文件夹加进它的排除列表，大存档会快很多。",
  "guide.trouble.p4":
    "“查看详情”会打开刚才那次操作的完整日志。反馈问题时，这份日志最有用。",
}

const translations: Record<Locale, Record<TranslationKey, string>> = {
  en,
  "zh-CN": zhCN,
}

interface I18nContextValue {
  locale: Locale
  setLocale: (locale: Locale) => void
  t: (key: TranslationKey, values?: Record<string, string | number | undefined>) => string
}

const I18nContext = createContext<I18nContextValue | null>(null)

function initialLocale(): Locale {
  const savedLocale = window.localStorage.getItem(LOCALE_STORAGE_KEY)
  return savedLocale === "zh-CN" ? "zh-CN" : "en"
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocale] = useState<Locale>(initialLocale)

  useEffect(() => {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, locale)
    document.documentElement.lang = locale
  }, [locale])

  const t = useCallback(
    (key: TranslationKey, values: Record<string, string | number | undefined> = {}) =>
      translations[locale][key].replace(/\{(\w+)\}/g, (match, name: string) =>
        values[name] === undefined ? match : String(values[name])
      ),
    [locale]
  )

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, t])

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

export function useI18n() {
  const context = useContext(I18nContext)
  if (!context) throw new Error("useI18n must be used within an I18nProvider")
  return context
}
