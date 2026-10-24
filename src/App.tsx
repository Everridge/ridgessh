import { useEffect, useRef } from "react";
import type { PanelImperativeHandle } from "react-resizable-panels";
import { AppSidebar } from "@/components/layout/app-sidebar";
import { TopBar } from "@/components/layout/top-bar";
import { HostDetailsPanel } from "@/features/hosts/host-details-panel";
import { HostListPanel } from "@/features/hosts/host-list-panel";
import { HostFormDialog } from "@/features/hosts/host-form-dialog";
import { BackupsDialog } from "@/features/backup/backups-dialog";
import { RestoreDialog } from "@/features/backup/restore-dialog";
import { ConfigWriteDialog } from "@/features/config-write/config-write-dialog";
import { HistoryDialog } from "@/features/history/history-dialog";
import { RunScriptDialog } from "@/features/scripts/run-script-dialog";
import { ExportDialog } from "@/features/export-import/export-dialog";
import { ImportDialog } from "@/features/export-import/import-dialog";
import { ScriptsDialog } from "@/features/scripts/scripts-dialog";
import { SettingsDialog } from "@/features/settings/settings-dialog";
import { UpdateDialog } from "@/features/updates/update-dialog";
import { useStartupUpdateCheck } from "@/features/updates/use-startup-update-check";
import { TransferDialog } from "@/features/transfer/transfer-dialog";
import { SyncReviewDialog } from "@/features/sync/sync-review-dialog";
import { useStartupSync } from "@/features/sync/use-startup-sync";
import { QuickConnect } from "@/features/terminal/quick-connect";
import { useQuickConnectShortcut } from "@/features/terminal/use-quick-connect-shortcut";
import { useTerminalStore } from "@/features/terminal/store";
import { TerminalPanel } from "@/features/terminal/terminal-panel";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";

/** Expands the collapsed terminal panel whenever a new session tab opens. */
function useExpandTerminalOnOpen() {
  const panel = useRef<PanelImperativeHandle | null>(null);
  const tabCount = useTerminalStore((s) => s.tabs.length);
  const prev = useRef(tabCount);
  useEffect(() => {
    if (tabCount > prev.current && panel.current?.isCollapsed()) panel.current.expand();
    prev.current = tabCount;
  }, [tabCount]);
  return panel;
}

export default function App() {
  useStartupSync();
  useStartupUpdateCheck();
  useQuickConnectShortcut();
  const terminalPanel = useExpandTerminalOnOpen();

  return (
    <SidebarProvider className="h-svh overflow-hidden">
      <AppSidebar />
      <SidebarInset className="min-w-0">
        <TopBar />
        <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1">
          <ResizablePanel defaultSize="55" minSize="20">
            <ResizablePanelGroup orientation="horizontal">
              <ResizablePanel defaultSize="62" minSize="30">
                <HostListPanel />
              </ResizablePanel>
              <ResizableHandle />
              <ResizablePanel defaultSize="38" minSize="20">
                <HostDetailsPanel />
              </ResizablePanel>
            </ResizablePanelGroup>
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel
            panelRef={terminalPanel}
            defaultSize="45"
            minSize="10"
            collapsible
            collapsedSize="0"
          >
            <TerminalPanel />
          </ResizablePanel>
        </ResizablePanelGroup>
      </SidebarInset>
      <HostFormDialog />
      <SyncReviewDialog />
      <ConfigWriteDialog />
      <BackupsDialog />
      <RestoreDialog />
      <SettingsDialog />
      <HistoryDialog />
      <TransferDialog />
      <ScriptsDialog />
      <RunScriptDialog />
      <ExportDialog />
      <ImportDialog />
      <UpdateDialog />
      <QuickConnect />
    </SidebarProvider>
  );
}
