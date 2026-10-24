import {
  useFieldArray,
  useForm,
  useWatch,
  type Control,
  type FieldErrors,
  type UseFormRegister,
} from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { InfoIcon, PlusIcon, StarIcon, XIcon } from "lucide-react";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { GroupSelect } from "@/features/groups/group-select";
import type { Host } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { hostFormSchema, toFormValues, toHostInput, type HostFormValues } from "./host-form";
import { useCreateHost, useHostsData, useUpdateHost } from "./queries";
import { TagInput } from "./tag-input";

/** Create/edit dialog, driven by `useUiStore().hostForm`. */
export function HostFormDialog() {
  const target = useUiStore((s) => s.hostForm);
  const close = useUiStore((s) => s.closeHostForm);
  const hosts = useHostsData();
  const host = target && target !== "new" ? hosts.find((h) => h.id === target) : undefined;

  return (
    <Dialog open={target !== null} onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-2xl">
        {target !== null && <HostForm key={target} host={host} onClose={close} />}
      </DialogContent>
    </Dialog>
  );
}

function HostForm({ host, onClose }: { host?: Host; onClose: () => void }) {
  const ui = useUiStore.getState();
  const selectHost = useUiStore((s) => s.selectHost);
  const create = useCreateHost();
  const update = useUpdateHost();
  const form = useForm<HostFormValues>({
    resolver: zodResolver(hostFormSchema),
    defaultValues: toFormValues(host, { groupId: ui.view === "group" ? ui.groupId : null }),
  });
  const { register, handleSubmit, control, setValue, formState } = form;
  const [groupId, tags, favorite] = useWatch({ control, name: ["groupId", "tags", "favorite"] });
  const errors = formState.errors;
  const pending = create.isPending || update.isPending;

  const submit = handleSubmit((values) => {
    const input = toHostInput(values, host?.color ?? null);
    if (host) {
      update.mutate({ id: host.id, input }, { onSuccess: onClose });
    } else {
      create.mutate(input, {
        onSuccess: (h) => {
          selectHost(h.id);
          onClose();
        },
      });
    }
  });

  const inConfig = host && host.syncState !== "db_only";
  const connectionErrors = ["alias", "hostname", "user", "port", "identityFile", "proxyJump"].some(
    (k) => k in errors,
  );

  return (
    <form onSubmit={submit} className="grid gap-4">
      <DialogHeader>
        <DialogTitle>{host ? `Edit ${host.alias}` : "New host"}</DialogTitle>
        <DialogDescription className="flex items-start gap-1.5">
          <InfoIcon className="mt-0.5 size-3.5 shrink-0" />
          {inConfig
            ? "Connection changes are saved here and queued for ~/.ssh/config. The config file is only written after you review and confirm."
            : "Saved in RidgeSSH only. You can add it to ~/.ssh/config later."}
        </DialogDescription>
      </DialogHeader>

      <Tabs defaultValue="connection">
        <TabsList>
          <TabsTrigger value="connection" className={connectionErrors ? "text-destructive" : ""}>
            Connection
          </TabsTrigger>
          <TabsTrigger value="info">Info</TabsTrigger>
          <TabsTrigger value="advanced" className={errors.extraOptions ? "text-destructive" : ""}>
            Advanced
          </TabsTrigger>
        </TabsList>

        <TabsContent value="connection" className="grid gap-3 pt-2 sm:grid-cols-6">
          <Field label="Alias (Host)" error={errors.alias?.message} className="sm:col-span-3">
            <Input id="alias" autoFocus placeholder="prod-db" {...register("alias")} />
          </Field>
          <Field label="Group" className="sm:col-span-3">
            <GroupSelect
              value={groupId}
              onChange={(g) => setValue("groupId", g, { shouldDirty: true })}
            />
          </Field>
          <Field label="Hostname" error={errors.hostname?.message} className="sm:col-span-4">
            <Input
              id="hostname"
              placeholder="10.0.0.5 or db.example.com"
              {...register("hostname")}
            />
          </Field>
          <Field label="Port" error={errors.port?.message} className="sm:col-span-2">
            <Input id="port" inputMode="numeric" placeholder="22" {...register("port")} />
          </Field>
          <Field label="User" error={errors.user?.message} className="sm:col-span-2">
            <Input id="user" placeholder="root" {...register("user")} />
          </Field>
          <Field
            label="Identity file"
            error={errors.identityFile?.message}
            className="sm:col-span-4"
          >
            <Input
              id="identityFile"
              placeholder="~/.ssh/id_ed25519"
              {...register("identityFile")}
            />
          </Field>
          <Field label="ProxyJump" error={errors.proxyJump?.message} className="sm:col-span-6">
            <Input id="proxyJump" placeholder="bastion" {...register("proxyJump")} />
          </Field>
        </TabsContent>

        <TabsContent value="info" className="grid gap-3 pt-2">
          <Field label="Description">
            <Input
              id="description"
              placeholder="What is this server for?"
              {...register("description")}
            />
          </Field>
          <Field label="Tags">
            <TagInput
              id="tags"
              value={tags}
              onChange={(t) => setValue("tags", t, { shouldDirty: true })}
            />
          </Field>
          <Field label="Notes">
            <Textarea id="notes" rows={5} placeholder="Markdown supported" {...register("notes")} />
          </Field>
          <PairList
            name="fields"
            title="Custom fields"
            keyPlaceholder="Name"
            valuePlaceholder="Value"
            control={control}
            register={register}
            errors={errors}
          />
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={favorite}
              onCheckedChange={(c) => setValue("favorite", c === true, { shouldDirty: true })}
            />
            <StarIcon className="size-4" /> Favorite
          </label>
        </TabsContent>

        <TabsContent value="advanced" className="grid gap-3 pt-2">
          <p className="text-sm text-muted-foreground">
            Other ssh_config options for this host, e.g. <code>ForwardAgent yes</code>,{" "}
            <code>LocalForward 8080 localhost:80</code>.
          </p>
          <PairList
            name="extraOptions"
            title="Options"
            keyPlaceholder="Option"
            valuePlaceholder="Value"
            control={control}
            register={register}
            errors={errors}
          />
        </TabsContent>
      </Tabs>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" disabled={pending}>
          {host ? "Save" : "Create host"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function Field({
  label,
  error,
  className,
  children,
}: {
  label: string;
  error?: string;
  className?: string;
  children: React.ReactElement<{ id?: string }>;
}) {
  return (
    <div className={`grid content-start gap-1.5 ${className ?? ""}`}>
      <Label htmlFor={children.props.id}>{label}</Label>
      {children}
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  );
}

interface PairListProps {
  name: "fields" | "extraOptions";
  title: string;
  keyPlaceholder: string;
  valuePlaceholder: string;
  control: Control<HostFormValues>;
  register: UseFormRegister<HostFormValues>;
  errors: FieldErrors<HostFormValues>;
}

function PairList({
  name,
  title,
  keyPlaceholder,
  valuePlaceholder,
  control,
  register,
  errors,
}: PairListProps) {
  const { fields, append, remove } = useFieldArray({ control, name });
  const error = errors[name]?.root?.message ?? errors[name]?.message;

  return (
    <div className="grid gap-1.5">
      <div className="flex items-center justify-between">
        <Label>{title}</Label>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          onClick={() => append({ key: "", value: "" })}
        >
          <PlusIcon /> Add
        </Button>
      </div>
      {fields.length === 0 && <p className="text-xs text-muted-foreground">None.</p>}
      {fields.map((f, i) => (
        <div key={f.id} className="flex gap-2">
          <Input className="w-2/5" placeholder={keyPlaceholder} {...register(`${name}.${i}.key`)} />
          <Input placeholder={valuePlaceholder} {...register(`${name}.${i}.value`)} />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label="Remove"
            onClick={() => remove(i)}
          >
            <XIcon />
          </Button>
        </div>
      ))}
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  );
}
