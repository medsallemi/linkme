import { invoke } from "@tauri-apps/api/core";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { UpdateSettings, useUpdateNotice } from "./Updates";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const update = {
  version: "0.10.0",
  publishedAt: "2026-11-20T12:00:00Z",
  notes: "<img src=x onerror=alert(1)>",
};
const status = {
  installed: "0.9.0",
  configured: true,
  automatic: true,
  lastChecked: 1795176000,
  release: update,
};

function Library({ blocked = false }: { blocked?: boolean }) {
  const { notice } = useUpdateNotice(blocked);
  return (
    <>
      <button>Library control</button>
      {notice}
    </>
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
});

it("defers for other workflows, renders text, and leaves dismissal scoped to one opening", async () => {
  let opening = 1;
  let offered = 0;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "check_for_updates")
      return { outcome: "available", opening, status };
    if (
      command === "claim_update_notice" &&
      args &&
      "opening" in args &&
      args.opening === opening &&
      offered !== opening
    ) {
      offered = opening;
      return status;
    }
    return null;
  });
  const view = render(<Library blocked />);
  expect(screen.queryByRole("dialog")).toBeNull();
  view.rerender(<Library />);
  const first = await screen.findByRole("dialog", {
    name: "A better Linkme awaits.",
  });
  expect(first.querySelector("img")).toBeNull();
  expect(first).toHaveProperty(
    "textContent",
    expect.stringContaining(update.notes),
  );
  expect(document.activeElement).toBe(
    within(first).getByRole("button", { name: "Remind me later" }),
  );
  fireEvent(first, new Event("cancel", { cancelable: true }));
  expect(screen.queryByRole("dialog")).toBeNull();
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(screen.queryByRole("dialog")).toBeNull();
  view.unmount();
  render(<Library />);
  await act(async () => {});
  expect(screen.queryByRole("dialog")).toBeNull();
  await act(async () => {
    opening++;
    window.dispatchEvent(new Event("linkme-library-open"));
  });
  const reopened = await screen.findByRole("dialog");
  fireEvent.click(
    within(reopened).getByRole("button", { name: "Close update" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("does no automatic work for hidden background startup", async () => {
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  render(<Library />);
  await act(async () => {});
  expect(invoke).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("clears an open offer on minimize and only offers again after restore", async () => {
  let opening = 1;
  let offered = 0;
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "check_for_updates")
      return { outcome: "available", opening, status };
    if (command === "claim_update_notice" && offered !== opening) {
      offered = opening;
      return status;
    }
    return null;
  });
  render(<Library />);
  await screen.findByRole("dialog");
  const focused = vi.mocked(document.hasFocus);
  focused.mockReturnValue(false);
  await act(async () =>
    window.dispatchEvent(new Event("linkme-library-minimized")),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(offered).toBe(1);
  opening++;
  focused.mockReturnValue(true);
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(await screen.findByRole("dialog")).toBeTruthy();
  expect(offered).toBe(2);
});

it("keeps manual checking and release notes available with automatic checking off", async () => {
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "update_preferences") {
      return {
        ...status,
        automatic: args && "enabled" in args ? args.enabled : true,
      };
    }
    if (command === "check_for_updates")
      return { outcome: "available", status };
    if (command === "open_update_release") return undefined;
    throw new Error("Unexpected IPC");
  });
  render(<UpdateSettings />);
  const toggle = await screen.findByRole("checkbox", {
    name: "Automatic update checks",
  });
  await act(async () => fireEvent.click(toggle));
  expect(invoke).toHaveBeenCalledWith("update_preferences", { enabled: false });
  expect((toggle as HTMLInputElement).checked).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
  expect(await screen.findByText("A new version is available.")).toBeTruthy();
  expect(invoke).toHaveBeenCalledWith("check_for_updates", { manual: true });
  fireEvent.click(screen.getByRole("button", { name: "View release notes" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("open_update_release"),
  );
});

it("contains keyboard focus and hands Update now to the restricted native action", async () => {
  let claimed = false;
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "check_for_updates")
      return { outcome: "available", opening: 1, status };
    if (command === "claim_update_notice") {
      if (claimed) return null;
      claimed = true;
      return status;
    }
    if (command === "open_update_release") return undefined;
    throw new Error("Unexpected IPC");
  });
  const view = render(<Library blocked />);
  const libraryControl = screen.getByRole("button", {
    name: "Library control",
  });
  libraryControl.focus();
  view.rerender(<Library />);
  const dialog = await screen.findByRole("dialog");
  const last = within(dialog).getByRole("button", { name: "Update now" });
  const first = within(dialog).getByRole("button", { name: "Close update" });
  for (const button of within(dialog).getAllByRole("button")) {
    vi.spyOn(button, "getClientRects").mockReturnValue({
      length: 1,
    } as DOMRectList);
  }
  last.focus();
  fireEvent.keyDown(last, { key: "Tab" });
  expect(document.activeElement).toBe(first);
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(last);
  await act(async () => fireEvent.click(last));
  expect(invoke).toHaveBeenCalledWith("open_update_release");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(libraryControl);
});

it.each([
  ["current", "You're up to date."],
  ["failed", "Unable to check for updates. Please try again."],
] as const)("explains the manual %s outcome", async (outcome, message) => {
  const noRelease = { ...status, release: null };
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "update_preferences") return noRelease;
    return { outcome, status: noRelease };
  });
  render(<UpdateSettings />);
  await screen.findByText("Installed version 0.9.0");
  fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
  expect(await screen.findByText(message)).toBeTruthy();
});

it("explains why the check is unavailable in an unconfigured build", async () => {
  vi.mocked(invoke).mockResolvedValue({
    ...status,
    configured: false,
    release: null,
  });
  render(<UpdateSettings />);
  await screen.findByText(
    "This build has no configured GitHub release source.",
  );
  const button = screen.getByRole("button", { name: "Check for updates" });
  expect(button.getAttribute("aria-disabled")).toBe("true");
  fireEvent.click(button);
  expect(invoke).toHaveBeenCalledTimes(1);
});
