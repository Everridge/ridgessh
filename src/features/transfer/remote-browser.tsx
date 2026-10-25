import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowUpIcon, FileIcon, FolderIcon, LoaderCircleIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { api, errorMessage } from "@/lib/ipc";
import { parentPath } from "./folder-plan";

const join = (dir: string, name: string) => `${dir.replace(/\/+$/, "")}/${name}`;

/** Picks a folder on a server by browsing it (key-based logins; the path can always be typed). */
export function RemoteBrowser({
  open,
  onOpenChange,
  hostId,
  user,
  start,
  title,
  onChoose,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hostId: string;
  user: string | null;
  /** Where to start: the typed path, or "" for the home folder. */
  start: string;
  title: string;
  onChoose: (path: string) => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[80vh] flex-col gap-3 sm:max-w-lg">
        {open && (
          <Body
            hostId={hostId}
            user={user}
            start={start}
            title={title}
            onChoose={(p) => {
              onChoose(p);
              onOpenChange(false);
            }}
            onCancel={() => onOpenChange(false)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function Body({
  hostId,
  user,
  start,
  title,
  onChoose,
  onCancel,
}: {
  hostId: string;
  user: string | null;
  start: string;
  title: string;
  onChoose: (path: string) => void;
  onCancel: () => void;
}) {
  const [path, setPath] = useState(start.trim());
  const listing = useQuery({
    queryKey: ["remote-browse", hostId, user, path],
    queryFn: () => api.folderBrowse(hostId, user, path),
    retry: false,
  });
  const data = listing.data;

  return (
    <>
      <DialogHeader>
        <DialogTitle>{title}</DialogTitle>
        <DialogDescription className="font-mono text-xs break-all">
          {data?.path ?? (path || "~")}
        </DialogDescription>
      </DialogHeader>

      <div className="flex items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={!data || data.path === "/"}
          onClick={() => data && setPath(parentPath(data.path))}
        >
          <ArrowUpIcon /> Up
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setPath("")}>
          Home
        </Button>
        {listing.isFetching && <LoaderCircleIcon className="size-4 animate-spin" />}
      </div>

      <ScrollArea className="h-80 rounded-md border">
        {listing.isError ? (
          <p className="p-3 text-sm text-destructive" role="alert">
            {errorMessage(listing.error)}
          </p>
        ) : (
          <ul aria-label="Folder contents" className="p-1 text-sm">
            {data?.entries.map((e) =>
              e.dir ? (
                <li key={e.name}>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded px-2 py-1 text-left hover:bg-muted"
                    onClick={() => setPath(join(data.path, e.name))}
                  >
                    <FolderIcon className="size-4 shrink-0 text-sky-600" />
                    <span className="truncate">{e.name}</span>
                  </button>
                </li>
              ) : (
                <li
                  key={e.name}
                  className="flex items-center gap-2 px-2 py-1 text-muted-foreground"
                >
                  <FileIcon className="size-4 shrink-0" />
                  <span className="truncate">{e.name}</span>
                </li>
              ),
            )}
            {data && data.entries.length === 0 && (
              <li className="px-2 py-1 text-muted-foreground">Empty folder</li>
            )}
            {data?.truncated && (
              <li className="px-2 py-1 text-xs text-muted-foreground">
                Only the first 2000 entries are shown.
              </li>
            )}
          </ul>
        )}
      </ScrollArea>

      <DialogFooter>
        <Button variant="outline" onClick={onCancel}>
          Cancel
        </Button>
        <Button disabled={!data} onClick={() => data && onChoose(data.path)}>
          Choose this folder
        </Button>
      </DialogFooter>
    </>
  );
}
