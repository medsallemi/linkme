import { useEffect, useRef, useState } from "react";

const exitDuration = 110;

export function usePopupDismiss(
  close: () => Promise<void>,
  onError: (error: unknown) => void,
) {
  const [exiting, setExiting] = useState(false);
  const closing = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => () => clearTimeout(timer.current), []);

  function beginDismiss(animate: boolean) {
    if (closing.current) return;
    closing.current = true;
    setExiting(true);
    const finish = () => {
      void close().catch((error: unknown) => {
        closing.current = false;
        setExiting(false);
        onError(error);
      });
    };
    if (
      animate &&
      window.matchMedia?.("(prefers-reduced-motion: no-preference)").matches
    ) {
      timer.current = setTimeout(finish, exitDuration);
    } else {
      finish();
    }
  }

  return {
    dismiss: () => beginDismiss(true),
    finishHandoff: () => beginDismiss(false),
    exiting,
  };
}
