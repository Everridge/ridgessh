import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@/components/ui/button";

interface State {
  error: Error | null;
}

/** Last-resort boundary so a render error shows a message instead of a blank window. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Unhandled UI error", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="flex h-svh flex-col items-center justify-center gap-4 p-8 text-center">
        <h1 className="text-lg font-semibold">Something went wrong</h1>
        <pre className="max-w-2xl overflow-auto rounded-md bg-muted p-3 text-left text-xs whitespace-pre-wrap">
          {this.state.error.message}
        </pre>
        <Button onClick={() => window.location.reload()}>Reload</Button>
      </div>
    );
  }
}
