// latest.json for the GitHub release the release workflow publishes.
//
//   node scripts/publish.mjs manifest <version> <installer.exe> <repo>
//
// latest.json is what installed apps read on startup (the updater endpoint in tauri.conf.json):
// the version, the CHANGELOG.md notes, and per platform the installer's URL and signature.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./version.mjs", import.meta.url));
const notes = (version) =>
  execFileSync(process.execPath, [script, "notes", version], { encoding: "utf8" }).trim();

const [cmd, version, ...rest] = process.argv.slice(2);

if (cmd === "manifest") {
  const [installer, repo] = rest;
  const url = `https://github.com/${repo}/releases/download/v${version}/${basename(installer)}`;
  const windows = { signature: readFileSync(`${installer}.sig`, "utf8").trim(), url };
  const manifest = {
    version,
    notes: notes(version),
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: { "windows-x86_64": windows, "windows-x86_64-nsis": windows },
  };
  console.log(JSON.stringify(manifest, null, 2));
} else {
  console.error("usage: publish.mjs manifest <version> <installer.exe> <repo>");
  process.exit(2);
}
