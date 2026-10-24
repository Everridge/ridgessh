import { AppSidebar } from "@/components/layout/app-sidebar";
import { TopBar } from "@/components/layout/top-bar";
import { HostDetailsPanel } from "@/features/hosts/host-details-panel";
import { HostListPanel } from "@/features/hosts/host-list-panel";
import { TerminalPanel } from "@/features/terminal/terminal-panel";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";

export default function App() {
  return (
    <SidebarProvider className="h-svh overflow-hidden">
      <AppSidebar />
      <SidebarInset className="min-w-0">
        <TopBar />
        <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1">
          <ResizablePanel defaultSize="65" minSize="25">
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
          <ResizablePanel defaultSize="35" minSize="10" collapsible collapsedSize="0">
            <TerminalPanel />
          </ResizablePanel>
        </ResizablePanelGroup>
      </SidebarInset>
    </SidebarProvider>
  );
}
