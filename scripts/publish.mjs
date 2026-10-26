// Files the release workflow publishes to the public releases repo.
//
//   node scripts/publish.mjs manifest <version> <installer.exe> <repo>  print latest.json
//   node scripts/publish.mjs readme <version> <repo>                    print the README
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

/** The installer under a name that stays the same, for "latest" download links. */
export const STABLE_INSTALLER = "RidgeSSH-setup.exe";

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
} else if (cmd === "readme") {
  const [repo] = rest;
  const latest = `https://github.com/${repo}/releases/latest/download/${STABLE_INSTALLER}`;
  console.log(`# RidgeSSH

A desktop SSH connection manager for Windows, built on top of your \`~/.ssh/config\`.

## Download

**[Download RidgeSSH ${version} for Windows](${latest})** (Windows 10 and 11, 64-bit)

The installer needs no administrator rights. Once installed, RidgeSSH checks for a new version
when it starts and offers to update itself.

Older versions: [all releases](https://github.com/${repo}/releases).

## What's new in ${version}

${notes(version)}
`);
} else {
  console.error(
    "usage: publish.mjs manifest <version> <installer.exe> <repo> | readme <version> <repo>",
  );
  process.exit(2);
}
