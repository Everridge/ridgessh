import { useId, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { BookmarkIcon, FolderOpenIcon, FolderSearchIcon } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { keys, useHostsData, useVariants, variantsFor } from "@/features/hosts/queries";
import { UserSelect } from "@/features/hosts/user-select";
import { useTerminalStore } from "@/features/terminal/store";
import {
  api,
  errorMessage,
  type ExistingPolicy,
  type FolderSpec,
  type Host,
  type UserVariant,
} from "@/lib/ipc";
import { useUiStore, type FolderMode } from "@/stores/ui";
import { ExcludesInput, HookPicker } from "./folder-options";
import {
  EXISTING_LABELS,
  folderName,
  hookReady,
  hookToSpec,
  initialFor,
  initialFromSpec,
  parseExcludes,
  planSteps,
  tabTitle,
  validOwner,
  type FolderInitial,
} from "./folder-plan";
import { RemoteBrowser } from "./remote-browser";

const TITLES: Record<FolderMode, string> = {
  download: "Download folder",
  upload: "Upload folder",
  copy: "Copy folder between servers",
};
const START: Record<FolderMode, string> = { download: "Download", upload: "Upload", copy: "Copy" };

export function FolderDialog() {
  const target = useUiStore((s) => s.folderTransfer);
  const close = useUiStore((s) => s.closeFolderTransfer);
  const hosts = useHostsData();
  const initial: FolderInitial | null = !target
    ? null
    : target.recipe
      ? initialFromSpec(target.recipe.spec)
      : initialFor(target.mode, target.hostId, target.user ?? null);
  const host = hosts.find((h) => h.id === initial?.hostId);
  const recipe = target?.recipe ? { id: target.recipe.id, name: target.recipe.name } : null;
  return (
    <Dialog open={!!initial && !!host} onOpenChange={(o) => !o && close()}>
      <DialogContent className="flex max-h-[90vh] flex-col gap-3 sm:max-w-2xl">
        {initial && host && (
          <Body
            key={recipe?.id ?? `${host.id}:${initial.mode}`}
            host={host}
            initial={initial}
            recipe={recipe}
            onDone={close}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

const useLast = (hostId: string | null) =>
  useQuery({
    queryKey: ["folder-last", hostId],
    queryFn: () => api.folderLast(hostId!),
    enabled: !!hostId,
  }).data;

/** Which remote path field a folder browser fills in. */
type Browsing = { field: "src" | "dst"; hostId: string; user: string | null; start: string };

function Body({
  host,
  initial,
  recipe: initialRecipe,
  onDone,
}: {
  host: Host;
  initial: FolderInitial;
  recipe: { id: string; name: string } | null;
  onDone: () => void;
}) {
  const qc = useQueryClient();
  const hosts = useHostsData();
  const variants = useVariants();
  const [mode, setMode] = useState(initial.mode);
  const [user, setUser] = useState(initial.user);
  const [otherId, setOtherId] = useState(initial.otherId);
  const [otherUser, setOtherUser] = useState(initial.otherUser);
  // Typed values; null = what was used last time.
  const [srcPath, setSrcPath] = useState(initial.srcPath);
  const [dstPath, setDstPath] = useState(initial.dstPath);
  const [localSrc, setLocalSrc] = useState(initial.localSrc);
  const [localDst, setLocalDst] = useState(initial.localDst);
  const [unpack, setUnpack] = useState(initial.unpack);
  const [keepZip, setKeepZip] = useState(initial.keepZip);
  const [keepCopy, setKeepCopy] = useState(initial.keepCopy);
  const [direct, setDirect] = useState(initial.direct);
  const [existing, setExisting] = useState<ExistingPolicy>(initial.existing);
  const [owner, setOwner] = useState(initial.owner);
  const [excludes, setExcludes] = useState(initial.excludes);
  const [sudoSource, setSudoSource] = useState(initial.sudoSource);
  const [sudoDest, setSudoDest] = useState(initial.sudoDest);
  const [before, setBefore] = useState(initial.before);
  const [after, setAfter] = useState(initial.after);
  const [recipe, setRecipe] = useState(initialRecipe);
  // Saving: the name being typed (null = not saving).
  const [recipeName, setRecipeName] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState<Browsing | null>(null);

  const last = useLast(host.id);
  const otherLast = useLast(otherId);
  const alias = useMemo(() => {
    const byId = new Map(hosts.map((h) => [h.id, h.alias]));
    return (id: string) => byId.get(id) ?? id;
  }, [hosts]);

  const remoteSrcPath = srcPath ?? last?.remote ?? "";
  const remoteDstPath = dstPath ?? (mode === "copy" ? otherLast?.dest : last?.dest) ?? "";
  const localDstPath = localDst ?? last?.local ?? "";
  const ownerValue = owner.trim();
  const remoteSource = mode !== "upload";
  const remoteDest = mode !== "download";

  const spec = ((): FolderSpec | null => {
    const me = { kind: "remote" as const, hostId: host.id, user };
    const base = {
      unpack: false,
      keepZip: true,
      keepCopy: null,
      direct: false,
      existing,
      owner: remoteDest && ownerValue ? ownerValue : null,
      excludes: parseExcludes(excludes),
      sudoSource: remoteSource && sudoSource,
      sudoDest: remoteDest && sudoDest,
      before: remoteSource ? hookToSpec(before) : null,
      after: remoteDest ? hookToSpec(after) : null,
    };
    switch (mode) {
      case "download":
        return {
          ...base,
          unpack,
          keepZip: unpack ? keepZip : true,
          source: { ...me, path: remoteSrcPath.trim() },
          destination: { kind: "local", path: localDstPath },
        };
      case "upload":
        return {
          ...base,
          source: { kind: "local", path: localSrc },
          destination: { ...me, path: remoteDstPath.trim() },
        };
      case "copy":
        if (!otherId) return null;
        return {
          ...base,
          keepCopy: direct ? null : keepCopy,
          direct,
          source: { ...me, path: remoteSrcPath.trim() },
          destination: {
            kind: "remote",
            hostId: otherId,
            user: otherUser,
            path: remoteDstPath.trim(),
          },
        };
    }
  })();

  const ready =
    !!spec &&
    spec.source.path !== "" &&
    spec.destination.path !== "" &&
    (!ownerValue || validOwner(ownerValue)) &&
    (direct || keepCopy !== "") &&
    (!remoteSource || hookReady(before)) &&
    (!remoteDest || hookReady(after));

  const pick = async (title: string, current: string) => {
    const dir = await openDialog({ directory: true, title, defaultPath: current || undefined });
    return typeof dir === "string" ? dir : null;
  };

  const start = () => {
    if (!spec) return;
    useTerminalStore
      .getState()
      .openJob(
        { id: host.id, alias: host.alias },
        { kind: "folder", spec, recipeId: recipe?.id ?? null },
        tabTitle(spec, alias),
      );
    onDone();
  };

  const saveRecipe = async (name: string) => {
    if (!spec) return;
    try {
      const saved = await api.recipeSave(recipe?.id ?? null, name, spec);
      setRecipe({ id: saved.id, name: saved.name });
      setRecipeName(null);
      void qc.invalidateQueries({ queryKey: keys.recipes });
      toast.success(recipe ? `Recipe "${saved.name}" updated` : `Saved as "${saved.name}"`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const name = spec ? folderName(spec.source.path) : "";
  const dstHint =
    name && remoteDstPath.trim()
      ? `The folder ends up as ${remoteDstPath.trim().replace(/\/+$/, "")}/${name}`
      : "The folder is placed inside this one; it is created if missing.";
  const destHost = mode === "copy" ? otherId : host.id;
  const destUser = mode === "copy" ? otherUser : user;

  return (
    <>
      <DialogHeader>
        <DialogTitle>{recipe ? `${TITLES[mode]}: ${recipe.name}` : TITLES[mode]}</DialogTitle>
        <DialogDescription>
          The folder is zipped where it is and carried over as one archive (much faster than many
          small files), with a checksum check. Runs in a terminal tab, so you can answer prompts
          there.
        </DialogDescription>
      </DialogHeader>

      <Tabs value={mode} onValueChange={(v) => setMode(v as FolderMode)}>
        <TabsList>
          <TabsTrigger value="download">Server → this PC</TabsTrigger>
          <TabsTrigger value="upload">This PC → server</TabsTrigger>
          <TabsTrigger value="copy">Server → server</TabsTrigger>
        </TabsList>
      </Tabs>

      <ScrollArea className="min-h-0 flex-1">
        <div className="grid gap-5 pr-3">
          <fieldset className="grid gap-3">
            <legend className="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
              From
            </legend>
            {mode === "upload" ? (
              <LocalFolder
                label="Folder on this PC"
                value={localSrc}
                placeholder="Choose the folder to upload"
                onPick={async () => {
                  const d = await pick("Folder to upload", localSrc);
                  if (d) setLocalSrc(d);
                }}
              />
            ) : (
              <>
                <ServerLine host={host} user={user} onUser={setUser} variants={variants} />
                <PathInput
                  label="Remote folder"
                  value={remoteSrcPath}
                  onChange={setSrcPath}
                  placeholder="/var/www/site or ~/app"
                  onBrowse={() =>
                    setBrowsing({ field: "src", hostId: host.id, user, start: remoteSrcPath })
                  }
                />
              </>
            )}
          </fieldset>

          <fieldset className="grid gap-3">
            <legend className="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
              To
            </legend>
            {mode === "download" && (
              <>
                <LocalFolder
                  label="Save to"
                  value={localDstPath}
                  placeholder="Choose a folder on this PC"
                  onPick={async () => {
                    const d = await pick("Save the folder to", localDstPath);
                    if (d) setLocalDst(d);
                  }}
                />
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox checked={unpack} onCheckedChange={(v) => setUnpack(v === true)} />
                  Unpack after downloading
                </label>
                {unpack && (
                  <label className="flex items-center gap-2 pl-6 text-sm">
                    <Checkbox checked={keepZip} onCheckedChange={(v) => setKeepZip(v === true)} />
                    Keep the .zip as well
                  </label>
                )}
              </>
            )}
            {mode === "upload" && (
              <ServerLine host={host} user={user} onUser={setUser} variants={variants} />
            )}
            {mode === "copy" && (
              <HostPicker
                hosts={hosts}
                value={otherId}
                onChange={(id) => {
                  setOtherId(id);
                  setOtherUser(null);
                  setDstPath(null);
                }}
                user={otherUser}
                onUser={setOtherUser}
                variants={variants}
              />
            )}
            {remoteDest && (
              <PathInput
                label="Destination folder on the server"
                value={remoteDstPath}
                onChange={setDstPath}
                placeholder="/var/www"
                hint={dstHint}
                onBrowse={
                  destHost
                    ? () =>
                        setBrowsing({
                          field: "dst",
                          hostId: destHost,
                          user: destUser,
                          start: remoteDstPath,
                        })
                    : undefined
                }
              />
            )}
            {(remoteDest || unpack) && <ExistingSelect value={existing} onChange={setExisting} />}
            {remoteDest && (
              <OwnerInput
                value={owner}
                onChange={setOwner}
                invalid={!!ownerValue && !validOwner(ownerValue)}
                sudo={sudoDest}
              />
            )}
            {mode === "copy" && (
              <label className="flex items-start gap-2 text-sm">
                <Checkbox
                  className="mt-0.5"
                  checked={direct}
                  onCheckedChange={(v) => setDirect(v === true)}
                />
                <span>
                  Stream directly between the servers
                  <span className="block text-xs text-muted-foreground">
                    Nothing is saved on this PC. Needs key-based logins on both servers: while
                    streaming there is no terminal for password prompts.
                  </span>
                </span>
              </label>
            )}
            {mode === "copy" && !direct && (
              <div className="grid gap-2">
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox
                    checked={keepCopy !== null}
                    onCheckedChange={(v) => setKeepCopy(v === true ? (last?.local ?? "") : null)}
                  />
                  Also keep the archive on this PC
                </label>
                {keepCopy !== null && (
                  <LocalFolder
                    label="Keep it in"
                    value={keepCopy}
                    placeholder="Choose a folder on this PC"
                    onPick={async () => {
                      const d = await pick("Keep the archive in", keepCopy);
                      if (d) setKeepCopy(d);
                    }}
                  />
                )}
              </div>
            )}
          </fieldset>

          <details className="group rounded-md border px-3 py-2">
            <summary className="cursor-pointer text-sm font-medium select-none">
              More options: leave out, sudo, scripts
            </summary>
            <div className="grid gap-4 pt-3">
              <ExcludesInput value={excludes} onChange={setExcludes} />
              {remoteSource && (
                <label className="flex items-start gap-2 text-sm">
                  <Checkbox
                    className="mt-0.5"
                    checked={sudoSource}
                    onCheckedChange={(v) => setSudoSource(v === true)}
                  />
                  <span>
                    Read the source folder with sudo
                    <span className="block text-xs text-muted-foreground">
                      For folders only root can read. sudo may ask for your password in the tab.
                    </span>
                  </span>
                </label>
              )}
              {remoteDest && (
                <label className="flex items-start gap-2 text-sm">
                  <Checkbox
                    className="mt-0.5"
                    checked={sudoDest}
                    onCheckedChange={(v) => setSudoDest(v === true)}
                  />
                  <span>
                    Unpack (and set the owner) with sudo
                    <span className="block text-xs text-muted-foreground">
                      For destinations only root can write, such as /var/www or /opt.
                    </span>
                  </span>
                </label>
              )}
              {remoteSource && (
                <HookPicker
                  label="Script before (on the source server)"
                  hint="Runs before zipping, e.g. dump a database or stop a service. A failure stops the transfer."
                  value={before}
                  onChange={setBefore}
                />
              )}
              {remoteDest && (
                <HookPicker
                  label="Script after (on the destination server)"
                  hint="Runs after unpacking, e.g. restart a service or run migrations."
                  value={after}
                  onChange={setAfter}
                />
              )}
            </div>
          </details>

          {spec && (
            <ol
              aria-label="What will happen"
              className="grid list-decimal gap-0.5 rounded-md border bg-muted/30 py-2 pr-3 pl-8 text-xs text-muted-foreground"
            >
              {planSteps(spec, alias).map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ol>
          )}
        </div>
      </ScrollArea>

      <DialogFooter className="items-center gap-2 sm:justify-between">
        {recipeName !== null ? (
          <form
            className="flex items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void saveRecipe(recipeName);
            }}
          >
            <Input
              aria-label="Recipe name"
              value={recipeName}
              onChange={(e) => setRecipeName(e.target.value)}
              placeholder="e.g. prod uploads → stage"
              className="h-8 w-56"
              autoFocus
            />
            <Button size="sm" type="submit" disabled={!recipeName.trim() || !spec}>
              Save
            </Button>
            <Button size="sm" variant="ghost" type="button" onClick={() => setRecipeName(null)}>
              Cancel
            </Button>
          </form>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            disabled={!ready}
            onClick={() => setRecipeName(recipe?.name ?? "")}
          >
            <BookmarkIcon /> {recipe ? "Update recipe…" : "Save as recipe…"}
          </Button>
        )}
        <div className="flex gap-2">
          <Button variant="outline" onClick={onDone}>
            Cancel
          </Button>
          <Button disabled={!ready} onClick={start}>
            {START[mode]}
          </Button>
        </div>
      </DialogFooter>

      {browsing && (
        <RemoteBrowser
          open
          onOpenChange={(o) => !o && setBrowsing(null)}
          hostId={browsing.hostId}
          user={browsing.user}
          start={browsing.start}
          title={browsing.field === "src" ? "Choose the folder to copy" : "Choose where to put it"}
          onChoose={(p) => (browsing.field === "src" ? setSrcPath(p) : setDstPath(p))}
        />
      )}
    </>
  );
}

/** The host the dialog is for, with a "Connect as" choice when it has user blocks. */
function ServerLine({
  host,
  user,
  onUser,
  variants,
}: {
  host: Host;
  user: string | null;
  onUser: (u: string | null) => void;
  variants: UserVariant[];
}) {
  return (
    <div className="grid gap-3">
      <p className="text-sm">
        Server: <span className="font-medium">{host.alias}</span>{" "}
        <span className="text-muted-foreground">{host.hostname}</span>
      </p>
      <UserSelect
        users={variantsFor(host, variants).map((v) => v.user)}
        value={user}
        onChange={onUser}
        defaultLabel={host.user ? `${host.user} (Host block)` : "Default (Host block)"}
      />
    </div>
  );
}

function HostPicker({
  hosts,
  value,
  onChange,
  user,
  onUser,
  variants,
}: {
  hosts: Host[];
  value: string | null;
  onChange: (id: string) => void;
  user: string | null;
  onUser: (u: string | null) => void;
  variants: UserVariant[];
}) {
  const id = useId();
  const live = hosts.filter((h) => !h.deletedAt).sort((a, b) => a.alias.localeCompare(b.alias));
  const chosen = live.find((h) => h.id === value);
  return (
    <div className="grid gap-3">
      <div className="grid gap-1.5">
        <Label htmlFor={id}>Destination server</Label>
        <Select value={value ?? ""} onValueChange={onChange}>
          <SelectTrigger id={id} className="w-full">
            <SelectValue placeholder="Choose a server" />
          </SelectTrigger>
          <SelectContent>
            {live.map((h) => (
              <SelectItem key={h.id} value={h.id}>
                {h.alias}
                <span className="text-muted-foreground">{h.hostname}</span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {chosen && (
        <UserSelect
          users={variantsFor(chosen, variants).map((v) => v.user)}
          value={user}
          onChange={onUser}
          defaultLabel={chosen.user ? `${chosen.user} (Host block)` : "Default (Host block)"}
        />
      )}
    </div>
  );
}

function PathInput({
  label,
  value,
  onChange,
  placeholder,
  hint,
  onBrowse,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  hint?: string;
  onBrowse?: () => void;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <div className="flex gap-2">
        <Input
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          className="font-mono"
          spellCheck={false}
        />
        {onBrowse && (
          <Button variant="outline" onClick={onBrowse} aria-label={`Browse: ${label}`}>
            <FolderSearchIcon /> Browse…
          </Button>
        )}
      </div>
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

function LocalFolder({
  label,
  value,
  placeholder,
  onPick,
}: {
  label: string;
  value: string;
  placeholder: string;
  onPick: () => void;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <div className="flex gap-2">
        <Input
          id={id}
          value={value}
          readOnly
          placeholder={placeholder}
          className="font-mono text-xs"
        />
        <Button variant="outline" onClick={onPick} aria-label={`Choose: ${label}`}>
          <FolderOpenIcon /> Choose…
        </Button>
      </div>
    </div>
  );
}

function ExistingSelect({
  value,
  onChange,
}: {
  value: ExistingPolicy;
  onChange: (v: ExistingPolicy) => void;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>If a folder of that name exists</Label>
      <Select value={value} onValueChange={(v) => onChange(v as ExistingPolicy)}>
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(Object.keys(EXISTING_LABELS) as ExistingPolicy[]).map((p) => (
            <SelectItem key={p} value={p}>
              {EXISTING_LABELS[p]}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function OwnerInput({
  value,
  onChange,
  invalid,
  sudo,
}: {
  value: string;
  onChange: (v: string) => void;
  invalid: boolean;
  sudo: boolean;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>Set owner (optional)</Label>
      <Input
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="www-data:www-data"
        className="max-w-xs font-mono"
        aria-invalid={invalid}
        spellCheck={false}
      />
      <p className="text-xs text-muted-foreground">
        {invalid
          ? "Use user or user:group."
          : sudo
            ? "Zip does not keep file owners; this sets them with sudo after unpacking."
            : "Zip does not keep file owners. Changing them needs root: connect as root, or turn on sudo under More options."}
      </p>
    </div>
  );
}
