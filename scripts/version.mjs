// Keeps the app version in package.json, Cargo.toml, Cargo.lock and tauri.conf.json in step.
//
//   node scripts/version.mjs bump 0.2.0   set the version everywhere
//   node scripts/version.mjs check v0.2.0 fail unless every file (and the tag) agree
//   node scripts/version.mjs notes 0.2.0  print that version's CHANGELOG.md section
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (f) => readFileSync(root + f, "utf8");
const write = (f, s) => writeFileSync(root + f, s);

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
const [cmd, arg] = process.argv.slice(2);

if (cmd === "bump") {
  if (!SEMVER.test(arg ?? "")) throw new Error(`not a version: ${arg}`);
  for (const [f, re] of Object.entries(files)) {
    write(
      f,
      read(f).replace(re, (all, old) => all.replace(`"${old}"`, `"${arg}"`)),
    );
  }
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
} else {
  console.error("usage: version.mjs bump <version> | check [tag] | notes <version>");
  process.exit(2);
}
