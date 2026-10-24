// jsdom reports every element as 0x0, so react-resizable-panels treats any pointer at (0,0)
// as hitting a separator and calls preventDefault() on pointerdown, which suppresses the
// mousedown that Radix tabs/menus rely on. Tests use plain containers instead.
export const ResizablePanelGroup = ({ children }: { children?: React.ReactNode }) => (
  <div>{children}</div>
);
export const ResizablePanel = ({ children }: { children?: React.ReactNode }) => (
  <div>{children}</div>
);
export const ResizableHandle = () => null;
