import type { ExistingPolicy, FolderEndpoint, FolderSpec } from "@/lib/ipc";

/** Last component of a path ("" for a remote "~", whose name depends on the server). */
export function folderName(path: string): string {
  const last =
    path
      .trim()
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() ?? "";
  return last === "~" ? "" : last;
}

export const EXISTING_LABELS: Record<ExistingPolicy, string> = {
  backup: "Move it aside to name.bak-<date> first",
  merge: "Unpack over it (keeps files that are not in the copy)",
  stop: "Stop without changing anything",
};

const where = (e: FolderEndpoint, alias: (id: string) => string) =>
  e.kind === "remote" ? (e.user ? `${e.user}@${alias(e.hostId)}` : alias(e.hostId)) : "this PC";

/** What the backend will do, in order (mirrors `folder::steps`). */
export function planSteps(spec: FolderSpec, alias: (id: string) => string): string[] {
  const src = spec.source;
  const dst = spec.destination;
  const out = [
    src.kind === "remote"
      ? `Check the folder on ${where(src, alias)}: size, free space in /tmp, zip available`
      : "Check the folder on this PC and free space for the archive",
  ];
  if (dst.kind === "remote")
    out.push(`Check ${where(dst, alias)}: unzip available, can write there, existing folder`);
  if (src.kind === "remote") {
    out.push(`Zip on ${where(src, alias)} into a private temporary file in /tmp`);
    out.push(
      dst.kind === "remote" && spec.keepCopy
        ? "Download to this PC (keeping a copy), compare SHA-256 checksums"
        : "Download to this PC, compare SHA-256 checksums",
    );
  } else {
    out.push("Zip on this PC into a temporary file");
  }
  if (dst.kind === "remote") {
    out.push(`Upload to ${where(dst, alias)}`);
    out.push("Verify the checksum and unpack on the server");
    if (spec.owner) out.push(`Set the owner to ${spec.owner}`);
  } else if (spec.unpack) {
    out.push(`Unpack here${spec.keepZip ? ", keeping the .zip" : " and delete the .zip"}`);
  }
  out.push("Clean up: delete the temporary archives");
  return out;
}

/** The terminal tab's title. */
export function tabTitle(spec: FolderSpec, alias: (id: string) => string): string {
  const name = folderName(spec.source.path) || "folder";
  const src = spec.source;
  const dst = spec.destination;
  if (src.kind === "remote" && dst.kind === "local") return `⇣ ${name} ← ${where(src, alias)}`;
  if (src.kind === "local" && dst.kind === "remote") return `⇡ ${name} → ${where(dst, alias)}`;
  return `⇄ ${name}: ${where(src, alias)} → ${where(dst, alias)}`;
}

/** `user`, `user:group` or numeric ids (mirrors `folder::check_owner`). */
export const validOwner = (owner: string) =>
  /^[A-Za-z0-9_.][A-Za-z0-9_.-]{0,31}(:[A-Za-z0-9_.][A-Za-z0-9_.-]{0,31})?$/.test(owner);
