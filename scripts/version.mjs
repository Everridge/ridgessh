// Keeps the app version in package.json, Cargo.toml, Cargo.lock and tauri.conf.json in step.
//
//   node scripts/version.mjs bump 0.2.0    set the version everywhere
//   node scripts/version.mjs check v0.2.0  fail unless every file (and the tag) agree
//   node scripts/version.mjs notes 0.2.0   print that version's CHANGELOG.md section
//   node scripts/version.mjs next          print the version the next release gets
//   node scripts/version.mjs prepare 0.2.0 bump, and give CHANGELOG.md a section for it
//
// The next release is the version in the files when that is newer than the last v* tag (set it
// with `bump` for a minor or major release). Otherwise it is the last tag with the patch number
// raised, or the minor/major one when a commit since that tag says #minor or #major.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (f) => readFileSync(root + f, "utf8");
const write = (f, s) => writeFileSync(root + f, s);
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();

// Each file: how to read its version, and how to replace it.
const files = {
  "package.json": /^ {2}"version": "([^"]+)"/m,
  "src-tauri/tauri.conf.json": /^ {2}"version": "([^"]+)"/m,
  "src-tauri/Cargo.toml": /^\[package\][^[]*?^version = "([^"]+)"/ms,
  "src-tauri/Cargo.lock": /^name = "ridgessh"\nversion = "([^"]+)"/m,
};

function versions() {
  return Object.entries(files).map(([f, re]) => {
    const m = read(f).match(re);
    if (!m) throw new Error(`no version found in ${f}`);
    return [f, m[1]];
  });
}

const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;
const parts = (v) => v.split(/[.-]/).slice(0, 3).map(Number);
const newer = (a, b) => {
  const [x, y] = [parts(a), parts(b)];
  const i = x.findIndex((n, k) => n !== y[k]);
  return i >= 0 && x[i] > y[i];
};

function bump(version) {
  if (!SEMVER.test(version ?? "")) throw new Error(`not a version: ${version}`);
  for (const [f, re] of Object.entries(files)) {
    write(
      f,
      read(f).replace(re, (all, old) => all.replace(`"${old}"`, `"${version}"`)),
    );
  }
}

/** The newest v* tag reachable from HEAD, without the v. */
function lastTag() {
  const tags = git("tag", "--merged", "HEAD", "--list", "v*")
    .split("\n")
    .map((t) => t.slice(1))
    .filter((t) => SEMVER.test(t));
  return tags.reduce((best, t) => (best === null || newer(t, best) ? t : best), null);
}

/** Subjects and bodies of the commits since the last release, oldest first. */
function commitsSince(tag) {
  const range = tag ? [`v${tag}..HEAD`] : ["HEAD"];
  const out = git("log", "--reverse", "--no-merges", "--format=%s%x1f%b%x1e", ...range);
  return out
    .split("\x1e")
    .map((c) => c.trim())
    .filter(Boolean)
    .map((c) => {
      const [subject, body = ""] = c.split("\x1f");
      return { subject: subject.trim(), body };
    })
    .filter((c) => !/^Release v\d/.test(c.subject));
}

function next() {
  const current = versions()[0][1];
  const tag = lastTag();
  if (!tag || newer(current, tag)) return current;
  const text = commitsSince(tag)
    .map((c) => `${c.subject}\n${c.body}`)
    .join("\n");
  const [major, minor, patch] = parts(tag);
  if (/#major\b/.test(text)) return `${major + 1}.0.0`;
  if (/#minor\b/.test(text)) return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

/**
 * Makes sure CHANGELOG.md has a "## <version>" section: an "## Unreleased" section is renamed,
 * and without either the commit subjects since the last release are listed.
 */
function prepareChangelog(version) {
  const lines = read("CHANGELOG.md").split(/\r?\n/);
  if (lines.some((l) => l === `## ${version}`)) return;
  const heading = (l) => l.startsWith("## ");
  const unreleased = lines.findIndex((l) => /^## unreleased\s*$/i.test(l));
  if (unreleased >= 0) {
    const end = lines.findIndex((l, i) => i > unreleased && heading(l));
    const body = lines.slice(unreleased + 1, end < 0 ? undefined : end);
    if (body.some((l) => l.trim())) {
      lines[unreleased] = `## ${version}`;
      write("CHANGELOG.md", lines.join("\n"));
      return;
    }
    lines.splice(unreleased, (end < 0 ? lines.length : end) - unreleased);
  }
  const subjects = commitsSince(lastTag()).map(
    (c) => `- ${c.subject.replace(/#(major|minor)\b/g, "").trim()}`,
  );
  const section = [
    `## ${version}`,
    "",
    ...(subjects.length ? subjects : ["- Maintenance release."]),
    "",
  ];
  const first = lines.findIndex(heading);
  lines.splice(first < 0 ? lines.length : first, 0, ...section);
  write("CHANGELOG.md", lines.join("\n"));
}

const [cmd, arg] = process.argv.slice(2);

if (cmd === "bump") {
  bump(arg);
  console.log(`version set to ${arg}`);
  if (!read("CHANGELOG.md").includes(`## ${arg}`)) {
    console.log(`add a "## ${arg}" section to CHANGELOG.md: it becomes the release notes`);
  }
} else if (cmd === "check") {
  const found = versions();
  const want = arg?.replace(/^v/, "") ?? found[0][1];
  const bad = found.filter(([, v]) => v !== want);
  for (const [f, v] of bad) console.error(`${f}: ${v}, expected ${want}`);
  if (bad.length) process.exit(1);
  console.log(`all files at ${want}`);
} else if (cmd === "notes") {
  const want = arg?.replace(/^v/, "");
  const lines = read("CHANGELOG.md").split(/\r?\n/);
  const start = lines.findIndex((l) => l.startsWith(`## ${want}`));
  if (start < 0) throw new Error(`CHANGELOG.md has no "## ${want}" section`);
  const end = lines.findIndex((l, i) => i > start && l.startsWith("## "));
  console.log(
    lines
      .slice(start + 1, end < 0 ? undefined : end)
      .join("\n")
      .trim(),
  );
} else if (cmd === "next") {
  console.log(next());
} else if (cmd === "prepare") {
  bump(arg);
  prepareChangelog(arg);
  console.log(`version set to ${arg}, CHANGELOG.md has its section`);
} else {
  console.error(
    "usage: version.mjs bump <version> | check [tag] | notes <version> | next | prepare <version>",
  );
  process.exit(2);
}
