import { useId, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpenIcon } from "lucide-react";
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
import { useHostsData, useVariants, variantsFor } from "@/features/hosts/queries";
import { UserSelect } from "@/features/hosts/user-select";
import { useTerminalStore } from "@/features/terminal/store";
import { api, type ExistingPolicy, type FolderSpec, type Host, type UserVariant } from "@/lib/ipc";
import { useUiStore, type FolderMode } from "@/stores/ui";
import { EXISTING_LABELS, folderName, planSteps, tabTitle, validOwner } from "./folder-plan";

const TITLES: Record<FolderMode, string> = {
  download: "Download folder",
  upload: "Upload folder",
  copy: "Copy folder between servers",
};
const START: Record<FolderMode, string> = { download: "Download", upload: "Upload", copy: "Copy" };

export function FolderDialog() {
  const target = useUiStore((s) => s.folderTransfer);
  const close = useUiStore((s) => s.closeFolderTransfer);
  const host = useHostsData().find((h) => h.id === target?.hostId);
  return (
    <Dialog open={!!target && !!host} onOpenChange={(o) => !o && close()}>
      <DialogContent className="flex max-h-[90vh] flex-col gap-3 sm:max-w-2xl">
        {target && host && (
          <Body
            key={`${host.id}:${target.mode}`}
            host={host}
            initialMode={target.mode}
            initialUser={target.user ?? null}
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

function Body({
  host,
  initialMode,
  initialUser,
  onDone,
}: {
  host: Host;
  initialMode: FolderMode;
  initialUser: string | null;
  onDone: () => void;
}) {
  const hosts = useHostsData();
  const variants = useVariants();
  const [mode, setMode] = useState(initialMode);
  // The host the dialog was opened for, and the second server when copying.
  const [user, setUser] = useState(initialUser);
  const [otherId, setOtherId] = useState<string | null>(null);
  const [otherUser, setOtherUser] = useState<string | null>(null);
  // Typed values; null = what was used last time.
  const [srcPath, setSrcPath] = useState<string | null>(null);
  const [dstPath, setDstPath] = useState<string | null>(null);
  const [localSrc, setLocalSrc] = useState("");
  const [localDst, setLocalDst] = useState<string | null>(null);
  const [unpack, setUnpack] = useState(false);
  const [keepZip, setKeepZip] = useState(true);
  const [keepCopy, setKeepCopy] = useState<string | null>(null);
  const [existing, setExisting] = useState<ExistingPolicy>("backup");
  const [owner, setOwner] = useState("");

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
  const remoteDest = mode !== "download";

  const spec = ((): FolderSpec | null => {
    const me = { kind: "remote" as const, hostId: host.id, user };
    const base = {
      unpack: false,
      keepZip: true,
      keepCopy: null,
      existing,
      owner: remoteDest && ownerValue ? ownerValue : null,
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
          keepCopy,
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
    keepCopy !== "";

  const pick = async (title: string, current: string) => {
    const dir = await openDialog({ directory: true, title, defaultPath: current || undefined });
    return typeof dir === "string" ? dir : null;
  };

  const start = () => {
    if (!spec) return;
    useTerminalStore
      .getState()
      .openJob({ id: host.id, alias: host.alias }, { kind: "folder", spec }, tabTitle(spec, alias));
    onDone();
  };

  const name = spec ? folderName(spec.source.path) : "";
  const dstHint =
    name && remoteDstPath.trim()
      ? `The folder ends up as ${remoteDstPath.trim().replace(/\/+$/, "")}/${name}`
      : "The folder is placed inside this one; it is created if missing.";

  return (
    <>
      <DialogHeader>
        <DialogTitle>{TITLES[mode]}</DialogTitle>
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
              />
            )}
            {(remoteDest || unpack) && <ExistingSelect value={existing} onChange={setExisting} />}
            {remoteDest && (
              <OwnerInput
                value={owner}
                onChange={setOwner}
                invalid={!!ownerValue && !validOwner(ownerValue)}
              />
            )}
            {mode === "copy" && (
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

      <DialogFooter>
        <Button variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button disabled={!ready} onClick={start}>
          {START[mode]}
        </Button>
      </DialogFooter>
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
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  hint?: string;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="font-mono"
        spellCheck={false}
      />
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
}: {
  value: string;
  onChange: (v: string) => void;
  invalid: boolean;
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
          : "Zip does not keep file owners. Changing the owner needs root on the server (sudo comes in a later version)."}
      </p>
    </div>
  );
}
