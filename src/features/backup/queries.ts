import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { invalidateAll, keys } from "@/features/hosts/queries";
import { api, errorMessage, type RestoreOptions, type Settings } from "@/lib/ipc";

export const useSettings = () => useQuery({ queryKey: keys.settings, queryFn: api.settingsGet });
export const useBackupStatus = () =>
  useQuery({ queryKey: keys.backupStatus, queryFn: api.backupStatus });
export const useBackups = () => useQuery({ queryKey: keys.backups, queryFn: api.backupsList });

function useRefreshBackups() {
  const qc = useQueryClient();
  return () => {
    for (const queryKey of [keys.settings, keys.backupStatus, keys.backups])
      void qc.invalidateQueries({ queryKey });
  };
}

export function useSaveSettings() {
  const refresh = useRefreshBackups();
  return useMutation({
    mutationFn: (s: Settings) => api.settingsUpdate(s),
    onSuccess: () => toast.success("Settings saved"),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: refresh,
  });
}

export function useRememberPassword() {
  const refresh = useRefreshBackups();
  return useMutation({
    mutationFn: (password: string) => api.backupPasswordRemember(password),
    onSuccess: () => toast.success("Backup password saved in Windows Credential Manager"),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: refresh,
  });
}

export function useForgetPassword() {
  const refresh = useRefreshBackups();
  return useMutation({
    mutationFn: api.backupPasswordForget,
    onSuccess: () => toast("Backup password forgotten. Automatic backups are off."),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: refresh,
  });
}

export function useCreateBackup() {
  const refresh = useRefreshBackups();
  return useMutation({
    mutationFn: (password?: string) => api.backupCreate(password),
    onSuccess: (b) => toast.success("Backup created", { description: b.fileName }),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: refresh,
  });
}

export function useDeleteBackup() {
  const refresh = useRefreshBackups();
  return useMutation({
    mutationFn: (fileName: string) => api.backupDelete(fileName),
    onSuccess: () => toast("Backup deleted"),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: refresh,
  });
}

/** Read-only: decrypts and compares. Errors are shown by the caller (wrong password etc.). */
export const useInspectBackup = () =>
  useMutation({
    mutationFn: ({ path, password }: { path: string; password: string }) =>
      api.backupInspect(path, password),
  });

export function useRestoreBackup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: { path: string; password: string; options: RestoreOptions }) =>
      api.backupRestore(v.path, v.password, v.options),
    onSuccess: (r) => qc.setQueryData(keys.lastSync, r.report),
    onSettled: () => {
      // Everything may have changed, settings included.
      void qc.invalidateQueries();
      invalidateAll(qc);
    },
  });
}

const REASONS: Record<string, string> = {
  manual: "Manual",
  "config-write": "Before writing the config",
  "before-restore": "Before a restore",
  "bulk-delete": "Before deleting hosts",
  sync: "Before applying a config scan",
  periodic: "Periodic",
  import: "Before an import",
  unknown: "Backup file",
};

export const reasonLabel = (reason: string) => REASONS[reason] ?? reason;

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
