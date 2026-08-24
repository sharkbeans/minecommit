/**
 * Check that nothing lets a player type a name for a world.
 *
 * A world carries the name it was first backed up under to every computer that
 * gets it. That only holds while there is exactly one place the name comes
 * from: the moment one machine can rename a world on the way down, the same
 * world exists under two names, no backup history connects them, and the
 * player is left with two entries that each look like a separate world.
 *
 * The rename fields are gone and both names are derived rather than held in
 * state. This makes sure they stay that way -- adding an input back is one
 * line, type-checks perfectly, and quietly undoes the guarantee.
 */
import { readFileSync } from "node:fs"

/**
 * The components that decide what a world is called, and the value in each
 * that holds its name. Scoped to the component rather than the file: these
 * files also carry fields for things a player may freely name, such as the
 * name recorded on their own backups.
 */
const GUARDED = [
  { file: "src/components/world-dialogs.tsx", component: "AddFromCloud", binding: "name" },
  { file: "src/components/cloud-setup.tsx", component: "PickStep", binding: "branch" },
]

/** `<Input ... value={x} ... />`, across however many lines it is written on. */
const INPUT = /<Input\b[^>]*?\bvalue=\{([^}]*)\}/gs

/** The body of a top-level `function name(`, up to its closing brace. */
function body(source, component) {
  const start = source.indexOf(`function ${component}(`)
  if (start === -1) {
    throw new Error(`${component} no longer exists; this check needs updating`)
  }
  const end = source.indexOf("\n}\n", start)
  return { start, text: source.slice(start, end === -1 ? undefined : end) }
}

const offenders = []
for (const { file, component, binding } of GUARDED) {
  const source = readFileSync(file, "utf8")
  const { start, text } = body(source, component)
  for (const [match, bound] of text.matchAll(INPUT)) {
    if (bound.trim() !== binding) continue
    const line = source.slice(0, start + text.indexOf(match)).split("\n").length
    offenders.push(`${file}:${line} — ${component} binds an editable field to \`${binding}\``)
  }
}

if (offenders.length > 0) {
  console.error(
    "A world's name must not be typed anywhere: it is the name every computer\n" +
      "that has the world will know it by, and a second name cannot be matched\n" +
      "back to the first.\n\n  " +
      offenders.join("\n  ")
  )
  process.exit(1)
}

console.log(`${GUARDED.length} naming flows checked; a world's name is never typed.`)
