import { z } from "zod";
import type { Host, HostInput } from "@/lib/ipc";

const configValue = (label: string, allowSpaces = false) =>
  z
    .string()
    .trim()
    .refine((v) => !v.startsWith("-"), `${label} cannot start with "-"`)
    // eslint-disable-next-line no-control-regex
    .refine((v) => !/[\x00-\x1f\x7f]/.test(v), `${label} contains invalid characters`)
    .refine((v) => allowSpaces || !/\s/.test(v), `${label} cannot contain spaces`);

const pair = z.object({ key: z.string(), value: z.string() });

/** A user name as `validate_user` accepts it: no spaces, `@`, wildcards or leading `-`. */
export const validUserName = (u: string) => u !== "" && !/^-|[\s@*?!"',]/.test(u);

/** One per-user block. Its other options are kept as they are (not edited here). */
const userMatch = z.object({
  user: z.string(),
  identityFile: z.string(),
  extraOptions: z.array(pair),
});

const userMatches = z.array(userMatch).superRefine((list, ctx) => {
  const seen = new Set<string>();
  list.forEach((m, i) => {
    const user = m.user.trim();
    const key = m.identityFile.trim();
    if (user === "" && key === "" && m.extraOptions.length === 0) return; // an empty row
    if (!validUserName(user)) {
      ctx.addIssue({
        code: "custom",
        path: [i, "user"],
        message: user === "" ? "Enter the user name" : "Not a valid user name",
      });
    } else if (seen.has(user)) {
      ctx.addIssue({ code: "custom", path: [i, "user"], message: `${user} is listed twice` });
    }
    seen.add(user);
    // eslint-disable-next-line no-control-regex
    if (key.startsWith("-") || /[\x00-\x1f\x7f]/.test(key))
      ctx.addIssue({ code: "custom", path: [i, "identityFile"], message: "Invalid identity file" });
  });
});

/** Mirrors the backend validation in `src-tauri/src/db/hosts.rs` for instant feedback. */
export const hostFormSchema = z.object({
  alias: configValue("Alias")
    .pipe(z.string().min(1, "Alias is required").max(255))
    .refine((v) => !/[*?!"',]/.test(v), "Alias cannot contain * ? ! , or quotes"),
  hostname: configValue("Hostname"),
  user: configValue("User"),
  port: z
    .string()
    .trim()
    .refine((v) => v === "" || (/^\d+$/.test(v) && +v >= 1 && +v <= 65535), "Port must be 1–65535"),
  identityFile: configValue("Identity file", true),
  proxyJump: configValue("ProxyJump"),
  groupId: z.string().nullable(),
  description: z.string(),
  notes: z.string(),
  favorite: z.boolean(),
  tags: z.array(z.string()),
  fields: z
    .array(pair)
    .refine(
      (fs) => fs.every((f) => f.key.trim() !== "" || f.value.trim() === ""),
      "Every custom field needs a name",
    ),
  userMatches,
  extraOptions: z
    .array(pair)
    .refine(
      (os) => os.every((o) => o.key.trim() === "" || /^[A-Za-z0-9]+$/.test(o.key.trim())),
      "Option names may only contain letters and digits",
    ),
});

export type HostFormValues = z.input<typeof hostFormSchema>;

export function toFormValues(h?: Host, defaults?: { groupId?: string | null }): HostFormValues {
  return {
    alias: h?.alias ?? "",
    hostname: h?.hostname ?? "",
    user: h?.user ?? "",
    port: h?.port?.toString() ?? "",
    identityFile: h?.identityFile ?? "",
    proxyJump: h?.proxyJump ?? "",
    groupId: h ? h.groupId : (defaults?.groupId ?? null),
    description: h?.description ?? "",
    notes: h?.notes ?? "",
    favorite: h?.favorite ?? false,
    tags: h?.tags.map((t) => t.name) ?? [],
    fields: h?.fields ?? [],
    extraOptions: h?.extraOptions ?? [],
    userMatches: (h?.userMatches ?? []).map((m) => ({
      user: m.user,
      identityFile: m.identityFile ?? "",
      extraOptions: m.extraOptions,
    })),
  };
}

const opt = (v: string) => (v.trim() === "" ? null : v.trim());

export function toHostInput(v: HostFormValues, color: string | null = null): HostInput {
  return {
    alias: v.alias.trim(),
    hostname: opt(v.hostname),
    user: opt(v.user),
    port: v.port.trim() === "" ? null : Number(v.port),
    identityFile: opt(v.identityFile),
    proxyJump: opt(v.proxyJump),
    extraOptions: v.extraOptions.filter((o) => o.key.trim() !== ""),
    userMatches: v.userMatches
      .filter((m) => m.user.trim() !== "" || m.identityFile.trim() !== "")
      .map((m) => ({
        user: m.user.trim(),
        identityFile: opt(m.identityFile),
        extraOptions: m.extraOptions,
      })),
    groupId: v.groupId,
    description: v.description,
    notes: v.notes,
    color,
    favorite: v.favorite,
    tags: v.tags,
    fields: v.fields.filter((f) => f.key.trim() !== "" || f.value.trim() !== ""),
  };
}

/** Same as `toHostInput` but from an existing host — for single-field edits. */
export function hostToInput(h: Host): HostInput {
  return toHostInput(toFormValues(h), h.color);
}
