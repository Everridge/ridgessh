import { useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  CheckCircle2Icon,
  FolderOpenIcon,
  KeyRoundIcon,
  RefreshCwIcon,
  TriangleAlertIcon,
} from "lucide-react";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { usePasswordFields } from "@/features/backup/password-fields";
import {
  useBackupStatus,
  useForgetPassword,
  useRememberPassword,
  useSaveSettings,
  useSettings,
} from "@/features/backup/queries";
import { useUpdateStore } from "@/features/updates/store";
import { TerminalSettings } from "./terminal-settings";
import { api, type Settings, type SettingsView } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { platform } from "@/lib/platform";

export function SettingsDialog() {
  const open = useUiStore((s) => s.settingsOpen);
  const setOpen = useUiStore((s) => s.setSettingsOpen);
  const view = useSettings().data;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex max-h-[85vh] flex-col gap-3 sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>
            Backups, the SSH config confirmation word, the terminal, updates and app info.
          </DialogDescription>
        </DialogHeader>
        {/* Re-mount the form whenever the saved settings change. */}
        {open && view && (
          <SettingsForm key={JSON.stringify(view)} view={view} onClose={() => setOpen(false)} />
        )}
      </DialogContent>
    </Dialog>
  );
}

function SettingsForm({ view, onClose }: { view: SettingsView; onClose: () => void }) {
  const [s, setS] = useState<Settings>(view.settings);
  const save = useSaveSettings();
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((x) => ({ ...x, [k]: v }));
  const dirty = JSON.stringify(s) !== JSON.stringify(view.settings);
  const folder = s.backupDir ?? view.defaultBackupDir;

  const pickFolder = async () => {
    const dir = await openDialog({ directory: true, defaultPath: folder, title: "Backup folder" });
    if (typeof dir === "string") set("backupDir", dir === view.defaultBackupDir ? null : dir);
  };

  return (
    <>
      <Tabs defaultValue="backups" className="min-h-0 flex-1">
        <TabsList>
          <TabsTrigger value="backups">Backups</TabsTrigger>
          <TabsTrigger value="config">Config & trash</TabsTrigger>
          <TabsTrigger value="terminal">Terminal</TabsTrigger>
          <TabsTrigger value="about">About</TabsTrigger>
        </TabsList>
        <ScrollArea className="min-h-0 flex-1">
          <TabsContent value="backups" className="grid gap-6 py-3 pr-3">
            <PasswordSection />

            <Section title="Folder">
              <div className="flex gap-2">
                <Input
                  value={folder}
                  readOnly
                  aria-label="Backup folder"
                  className="font-mono text-xs"
                />
                <Button variant="outline" onClick={() => void pickFolder()}>
                  Change…
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={`Show in ${platform.fileManager}`}
                  title={`Show in ${platform.fileManager}`}
                  onClick={() => void revealItemInDir(folder)}
                >
                  <FolderOpenIcon />
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                Backups are encrypted, so a OneDrive or Dropbox folder is fine.
                {s.backupDir && (
                  <>
                    {" "}
                    <button
                      type="button"
                      className="underline"
                      onClick={() => set("backupDir", null)}
                    >
                      Use the default folder
                    </button>
                  </>
                )}
              </p>
            </Section>

            <Section title="Automatic backups">
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={s.autoBackup}
                  onCheckedChange={(v) => set("autoBackup", v === true)}
                />
                Back up before writing the SSH config, restoring, deleting several hosts and
                applying config scans
              </label>
              <div className="grid gap-3 sm:grid-cols-3">
                <NumberField
                  label="Other edits: at most every (min)"
                  hint="0 = never"
                  value={s.autoBackupIntervalMin}
                  onChange={(v) => set("autoBackupIntervalMin", v)}
                  disabled={!s.autoBackup}
                />
                <NumberField
                  label="Keep automatic backups"
                  value={s.retentionCount}
                  onChange={(v) => set("retentionCount", v)}
                />
                <NumberField
                  label="Delete after (days)"
                  hint="0 = never; manual backups are always kept"
                  value={s.retentionDays}
                  onChange={(v) => set("retentionDays", v)}
                />
              </div>
            </Section>
          </TabsContent>

          <TabsContent value="config" className="grid gap-6 py-3 pr-3">
            <Section title="Trash">
              <div className="max-w-xs">
                <NumberField
                  label="Delete hosts from trash after (days)"
                  hint="0 = keep forever. Hosts still in ~/.ssh/config are never deleted automatically."
                  value={s.trashRetentionDays}
                  onChange={(v) => set("trashRetentionDays", v)}
                />
              </div>
            </Section>
            <Section title="Confirmation word">
              <Input
                value={s.confirmationWord}
                onChange={(e) => set("confirmationWord", e.target.value)}
                aria-label="Confirmation word"
                className="max-w-xs font-mono"
                spellCheck={false}
              />
              <p className="text-xs text-muted-foreground">
                Typed before RidgeSSH writes or restores ~/.ssh/config. Case-sensitive, 3–32
                characters, no spaces.
              </p>
            </Section>
          </TabsContent>

          <TabsContent value="terminal" className="py-3 pr-3">
            <TerminalSettings s={s} set={set} />
          </TabsContent>

          <TabsContent value="about" className="grid gap-6 py-3 pr-3">
            <About />
            <Section title="Updates">
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={s.checkUpdates}
                  onCheckedChange={(v) => set("checkUpdates", v === true)}
                />
                Check for updates when RidgeSSH starts
              </label>
              <UpdateCheck />
            </Section>
          </TabsContent>
        </ScrollArea>
      </Tabs>
      <DialogFooter>
        <Button variant="outline" onClick={onClose}>
          Close
        </Button>
        <Button disabled={!dirty || save.isPending} onClick={() => save.mutate(s)}>
          Save
        </Button>
      </DialogFooter>
    </>
  );
}

function PasswordSection() {
  const status = useBackupStatus().data;
  const remember = useRememberPassword();
  const forget = useForgetPassword();
  const { fields, password, reset } = usePasswordFields();
  const remembered = status?.passwordRemembered ?? false;

  return (
    <Section title="Password">
      {remembered ? (
        <p className="flex items-center gap-2 text-sm">
          <CheckCircle2Icon className="size-4 text-emerald-600" />
          Remembered in {platform.keychain}. Automatic backups use it.
        </p>
      ) : (
        <p className="flex items-center gap-2 text-sm text-amber-700 dark:text-amber-400">
          <TriangleAlertIcon className="size-4" />
          No password is remembered, so automatic backups are off.
        </p>
      )}
      {fields}
      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          disabled={!password || remember.isPending}
          onClick={() => password && remember.mutate(password, { onSuccess: reset })}
        >
          <KeyRoundIcon /> {remembered ? "Change password" : "Remember password"}
        </Button>
        {remembered && (
          <Button size="sm" variant="ghost" onClick={() => forget.mutate()}>
            Forget password
          </Button>
        )}
      </div>
      <p className="text-xs text-muted-foreground">
        Every backup is encrypted with the password it was made with. Changing the password does not
        re-encrypt older backups; keep the old password to restore them. There is no way to recover
        a forgotten password.
      </p>
    </Section>
  );
}

function About() {
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo }).data;
  if (!info) return null;
  const rows: [string, string][] = [
    ["Version", info.version],
    ["SSH config", info.sshConfig],
    ["Data folder", info.dataDir],
    ["Database", info.dbFile],
  ];
  return (
    <dl className="grid gap-2 text-sm">
      {rows.map(([k, v]) => (
        <div key={k} className="grid grid-cols-[8rem_1fr] gap-2">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="font-mono text-xs break-all">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

function UpdateCheck() {
  const { status, update, error, check, setDialogOpen } = useUpdateStore();
  const run = () =>
    check()
      .then((u) => u && setDialogOpen(true))
      .catch(() => undefined);
  return (
    <div className="flex flex-wrap items-center gap-3">
      <Button size="sm" variant="outline" disabled={status === "checking"} onClick={run}>
        <RefreshCwIcon className={status === "checking" ? "animate-spin" : undefined} />
        Check now
      </Button>
      <span className="text-sm text-muted-foreground" role="status">
        {status === "none" && "RidgeSSH is up to date."}
        {status === "error" && <span className="text-destructive">{error}</span>}
        {update && status !== "none" && status !== "error" && (
          <button type="button" className="underline" onClick={() => setDialogOpen(true)}>
            Version {update.version} is available
          </button>
        )}
      </span>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="grid gap-2">
      <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{title}</h3>
      {children}
    </section>
  );
}

function NumberField({
  label,
  hint,
  value,
  onChange,
  disabled,
}: {
  label: string;
  hint?: string;
  value: number;
  onChange: (v: number) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="grid content-start gap-1.5">
      <Label htmlFor={id} className="text-xs font-normal">
        {label}
      </Label>
      <Input
        id={id}
        type="number"
        min={0}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Math.max(0, Math.floor(Number(e.target.value) || 0)))}
      />
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}
