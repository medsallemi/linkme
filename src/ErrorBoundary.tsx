import { Component } from "react";
import type { ReactNode } from "react";

export class ErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (this.state.failed) {
      return (
        <main>
          <h1>Linkme could not display this window</h1>
          <p role="alert">
            Close and reopen Linkme. Committed links remain in your local
            database.
          </p>
        </main>
      );
    }
    return this.props.children;
  }
}
