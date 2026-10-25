import type { ExistingPolicy, FolderEndpoint, FolderHook, FolderSpec } from "@/lib/ipc";

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
  if (spec.before) out.push(`Run the script before on ${where(src, alias)}`);
  const excluded = spec.excludes?.length
    ? `, leaving out ${spec.excludes.length} pattern${spec.excludes.length === 1 ? "" : "s"}`
    : "";
  if (src.kind === "remote") {
    out.push(
      `Zip on ${where(src, alias)}${spec.sudoSource ? " as root" : ""} into a private temporary file in /tmp${excluded}`,
    );
    if (dst.kind === "remote" && spec.direct)
      out.push(
        `Stream from ${where(src, alias)} to ${where(dst, alias)} through this app (nothing is saved here), compare SHA-256 checksums`,
      );
    else
      out.push(
        dst.kind === "remote" && spec.keepCopy
          ? "Download to this PC (keeping a copy), compare SHA-256 checksums"
          : "Download to this PC, compare SHA-256 checksums",
      );
  } else {
    out.push(`Zip on this PC into a temporary file${excluded}`);
  }
  if (dst.kind === "remote") {
    if (!(src.kind === "remote" && spec.direct)) out.push(`Upload to ${where(dst, alias)}`);
    out.push(`Verify the checksum and unpack on the server${spec.sudoDest ? " as root" : ""}`);
    if (spec.owner) out.push(`Set the owner to ${spec.owner}`);
    if (spec.after) out.push(`Run the script after on ${where(dst, alias)}`);
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

/** The parent of an absolute remote path ("/" stays "/"). */
export function parentPath(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const i = trimmed.lastIndexOf("/");
  return i <= 0 ? "/" : trimmed.slice(0, i);
}

/** The "script before/after" picker's value. */
export type HookChoice =
  { kind: "none" } | { kind: "script"; scriptId: string } | { kind: "command"; command: string };

export function hookToSpec(c: HookChoice): FolderHook | null {
  if (c.kind === "script") return { scriptId: c.scriptId };
  if (c.kind === "command") return { command: c.command.trim() };
  return null;
}

export function hookFromSpec(h: FolderHook | null | undefined): HookChoice {
  if (h?.scriptId) return { kind: "script", scriptId: h.scriptId };
  if (h?.command != null) return { kind: "command", command: h.command };
  return { kind: "none" };
}

/** A command hook needs a command. */
export const hookReady = (c: HookChoice) => c.kind !== "command" || c.command.trim() !== "";

/** One exclude pattern per line (commas also separate). */
export const parseExcludes = (text: string) =>
  text
    .split(/[\r\n,]+/)
    .map((s) => s.trim())
    .filter(Boolean);

/** The folder dialog's starting values. */
export interface FolderInitial {
  mode: "download" | "upload" | "copy";
  /** The server the dialog is about (the source, or the destination for uploads). */
  hostId: string;
  user: string | null;
  /** The destination server when copying between servers. */
  otherId: string | null;
  otherUser: string | null;
  /** Remote paths; null = suggest the last one used. */
  srcPath: string | null;
  dstPath: string | null;
  localSrc: string;
  localDst: string | null;
  unpack: boolean;
  keepZip: boolean;
  keepCopy: string | null;
  direct: boolean;
  existing: ExistingPolicy;
  owner: string;
  excludes: string;
  sudoSource: boolean;
  sudoDest: boolean;
  before: HookChoice;
  after: HookChoice;
}

export function initialFor(
  mode: FolderInitial["mode"],
  hostId: string,
  user: string | null,
): FolderInitial {
  return {
    mode,
    hostId,
    user,
    otherId: null,
    otherUser: null,
    srcPath: null,
    dstPath: null,
    localSrc: "",
    localDst: null,
    unpack: false,
    keepZip: true,
    keepCopy: null,
    direct: false,
    existing: "backup",
    owner: "",
    excludes: "",
    sudoSource: false,
    sudoDest: false,
    before: { kind: "none" },
    after: { kind: "none" },
  };
}

/** A saved spec back into dialog values (`null` for a spec with no server, which cannot exist). */
export function initialFromSpec(spec: FolderSpec): FolderInitial | null {
  const src = spec.source;
  const dst = spec.destination;
  const common = {
    unpack: spec.unpack,
    keepZip: spec.keepZip,
    keepCopy: spec.keepCopy ?? null,
    direct: spec.direct ?? false,
    existing: spec.existing,
    owner: spec.owner ?? "",
    excludes: (spec.excludes ?? []).join("\n"),
    sudoSource: spec.sudoSource ?? false,
    sudoDest: spec.sudoDest ?? false,
    before: hookFromSpec(spec.before),
    after: hookFromSpec(spec.after),
  };
  if (src.kind === "remote" && dst.kind === "local")
    return {
      ...initialFor("download", src.hostId, src.user ?? null),
      ...common,
      srcPath: src.path,
      localDst: dst.path,
    };
  if (src.kind === "local" && dst.kind === "remote")
    return {
      ...initialFor("upload", dst.hostId, dst.user ?? null),
      ...common,
      localSrc: src.path,
      dstPath: dst.path,
    };
  if (src.kind === "remote" && dst.kind === "remote")
    return {
      ...initialFor("copy", src.hostId, src.user ?? null),
      ...common,
      otherId: dst.hostId,
      otherUser: dst.user ?? null,
      srcPath: src.path,
      dstPath: dst.path,
    };
  return null;
}
