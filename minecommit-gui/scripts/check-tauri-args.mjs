/**
 * Check that every `invoke("name", { ... })` in the frontend passes exactly the
 * arguments its Rust command declares.
 *
 * Tauri resolves these by name at runtime, so a typo or a renamed parameter
 * type-checks perfectly and then fails in front of the user with
 * "missing required key". Nothing else in the build catches it.
 */
import { readFileSync, readdirSync, statSync } from "node:fs"
import { join } from "node:path"

const LIB = "src-tauri/src/lib.rs"
const SRC = "src"

/** Arguments Tauri injects itself rather than taking from the caller. */
const INJECTED = new Set(["app", "state", "window", "webview"])

const camel = (name) =>
  name.split("_").map((part, i) => (i === 0 ? part : part[0].toUpperCase() + part.slice(1))).join("")

function commandSignatures() {
  // Line comments may sit between parameters, and may contain commas.
  const source = readFileSync(LIB, "utf8")
    .split("\n")
    .map((line) => line.split("//")[0])
    .join("\n")

  const signatures = new Map()
  const pattern = /#\[tauri::command\][\s\S]*?fn\s+(\w+)\s*\(([^)]*)\)/g
  for (const [, name, params] of source.matchAll(pattern)) {
    const args = params
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part.includes(":"))
      .map((part) => part.split(":")[0].trim())
      .filter((arg) => !INJECTED.has(arg))
    signatures.set(name, new Set(args.map(camel)))
  }
  return signatures
}

/** The opening and closing braces land in the first and last token. */
const clean = (token) => token.replace(/[{}]/g, "").trim()

/** Top-level keys of an object literal, ignoring anything nested in a value. */
function topLevelKeys(literal) {
  const keys = []
  let depth = 0
  let atKey = true
  let token = ""
  for (let i = 0; i < literal.length; i += 1) {
    const char = literal[i]
    if ("{[(`".includes(char)) depth += 1
    else if ("}])`".includes(char)) depth -= 1

    if (depth === 1 && char === ":" && atKey) {
      keys.push(clean(token))
      token = ""
      atKey = false
    } else if (depth === 1 && char === ",") {
      // A bare `{ name }` shorthand is a key in its own right.
      if (atKey && clean(token)) keys.push(clean(token))
      token = ""
      atKey = true
    } else if (atKey && depth === 1) {
      token += char
    }
  }
  if (atKey && clean(token)) keys.push(clean(token))
  return keys.filter((key) => /^\w+$/.test(key))
}

function* sourceFiles(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) yield* sourceFiles(path)
    else if (path.endsWith(".tsx") || path.endsWith(".ts")) yield path
  }
}

const signatures = commandSignatures()
const problems = []

for (const file of sourceFiles(SRC)) {
  const text = readFileSync(file, "utf8")
  const pattern = /invoke(?:<[\s\S]*?>)?\(\s*"(\w+)"\s*(?:,\s*(\{[\s\S]*?\n?\s*\})\s*)?\)/g
  for (const match of text.matchAll(pattern)) {
    const [, command, literal] = match
    const line = text.slice(0, match.index).split("\n").length
    const expected = signatures.get(command)
    if (!expected) {
      problems.push(`${file}:${line}  no #[tauri::command] named "${command}"`)
      continue
    }
    const passed = new Set(literal ? topLevelKeys(literal) : [])
    const missing = [...expected].filter((key) => !passed.has(key))
    const unknown = [...passed].filter((key) => !expected.has(key))
    if (missing.length || unknown.length) {
      problems.push(
        `${file}:${line}  ${command}` +
          (missing.length ? `\n    missing: ${missing.join(", ")}` : "") +
          (unknown.length ? `\n    not a parameter: ${unknown.join(", ")}` : "")
      )
    }
  }
}

if (problems.length) {
  console.error(`${problems.length} invoke call(s) do not match their command:\n`)
  console.error(problems.join("\n"))
  process.exit(1)
}
console.log(`${signatures.size} commands declared; every invoke matches.`)
