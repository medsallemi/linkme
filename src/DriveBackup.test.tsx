import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { DriveSettings, type DriveStatus } from "./DriveBackup";
import { Modal } from "./Modal";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const connected: DriveStatus = {
  configured: true,
  requiresReconnect: false,
  profile: {name:"Test account"},
  frequency:"off",
  lastBackup:null,
  warning:null,
};

it.each([undefined, connected])(
  "opens the fixed privacy policy without starting a Drive operation",
  async (status) => {
    vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
    render(<DriveSettings status={status} onChange={vi.fn()} />);
    const link = screen.getByRole("link", { name: "Linkme privacy policy" });
    expect(link.getAttribute("href")).toBe("https://linkme.bacontrack.tn/privacy");
    link.focus();
    expect(document.activeElement).toBe(link);
    fireEvent.click(link);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledExactlyOnceWith("open_drive_privacy_policy"),
    );
    vi.mocked(invoke).mockClear();
  },
);

it("reports the privacy policy opener error without altering the backup controls", async () => {
  vi.mocked(invoke).mockReset().mockRejectedValueOnce({
    code: "native_action",
    message: "Could not open the browser.",
  });
  render(<DriveSettings status={connected} onChange={vi.fn()} />);
  fireEvent.click(screen.getByRole("link", { name: "Linkme privacy policy" }));
  expect(await screen.findByRole("alert")).toHaveProperty(
    "textContent",
    "Could not open the browser.",
  );
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Back up now" }).disabled).toBe(false);
  vi.mocked(invoke).mockClear();
});

it("requires a configured Google client and connects without uploading", async () => {
  const change = vi.fn();
  const view = render(<DriveSettings status={{...connected, configured:false, profile:null}} onChange={change} />);
  const connect = screen.getByRole<HTMLButtonElement>("button", {name:"Connect Google Drive"});
  expect(connect.disabled).toBe(true);
  view.rerender(<DriveSettings status={{...connected, profile:null}} onChange={change} />);
  vi.mocked(invoke).mockResolvedValue(connected);
  fireEvent.click(connect);
  await waitFor(() => expect(change).toHaveBeenCalledWith(connected));
  expect(invoke).toHaveBeenCalledExactlyOnceWith("drive_backup",{action:{kind:"connect"}});
});

it("updates the schedule, keeping the previously confirmed state on failure", async () => {
  const change = vi.fn();
  vi.mocked(invoke).mockRejectedValueOnce({code:"native_action",message:"Please reconnect."});
  render(<DriveSettings status={connected} onChange={change} />);
  fireEvent.change(screen.getByLabelText("Automatic backups"), {target:{value:"weekly"}});
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Please reconnect.");
  expect(invoke).toHaveBeenLastCalledWith("drive_backup",{action:{kind:"schedule",frequency:"weekly"}});
  expect(change).not.toHaveBeenCalled();
});

it("prompts the existing account to reconnect before enabling backup and restore", async () => {
  const change = vi.fn();
  vi.mocked(invoke).mockReset().mockResolvedValue(connected);
  render(<DriveSettings status={{
    ...connected,
    requiresReconnect: true,
    warning: "Reconnect the same Google account to resume backups.",
  }} onChange={change} />);
  expect(screen.getByRole("alert").textContent).toContain("Reconnect the same Google account");
  expect(screen.getByRole<HTMLSelectElement>("combobox", { name: "Automatic backups" }).disabled).toBe(true);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Back up now" }).disabled).toBe(true);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: "Backup history…" }).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Reconnect" }));
  await waitFor(() => expect(change).toHaveBeenCalledWith(connected));
  expect(invoke).toHaveBeenCalledExactlyOnceWith("drive_backup", { action: { kind: "connect" } });
});

it("lists backups without changing the library and restores only after explicit confirmation", async () => {
  vi.mocked(invoke).mockClear().mockResolvedValueOnce([
    {id:"snapshot",name:"Linkme - 2026-11-23",createdTime:"2026-11-23T12:00:00Z",size:2048},
  ]).mockRejectedValueOnce({code:"invalid_backup",message:"This backup is invalid."});
  render(<DriveSettings status={connected} onChange={vi.fn()} />);
  fireEvent.click(screen.getByRole("button",{name:"Backup history…"}));
  const picker = await screen.findByLabelText("Backup to restore");
  const replace = screen.getByRole<HTMLButtonElement>("button",{name:"Replace library"});
  expect(replace.disabled).toBe(true);
  fireEvent.change(picker, {target:{value:"snapshot"}});
  expect(invoke).toHaveBeenCalledExactlyOnceWith("list_drive_backups");
  fireEvent.click(replace);
  await screen.findByText("This backup is invalid.");
  expect(invoke).toHaveBeenLastCalledWith("restore_drive_backup",{id:"snapshot"});
  expect(replace.disabled).toBe(false);
});

it("shows backup type and size, and signals completion only after restore commits", async () => {
  let finish = () => {};
  vi.mocked(invoke).mockReset().mockResolvedValueOnce([
    { id: "snapshot", name: "Internal filename", createdTime: "2026-11-23T12:00:00Z",
      size: 2048, kind: "Automatic", linkCount: 3 },
    { id: "legacy", name: "Old filename", createdTime: "2026-11-22T12:00:00Z", size: 1000 },
  ]).mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
  const restored = vi.fn();
  render(<DriveSettings status={connected} onChange={vi.fn()} onRestored={restored} />);
  fireEvent.click(screen.getByRole("button", { name: "Backup history…" }));
  await screen.findByRole("option", { name: /Automatic · 2 KB · 3 links/ });
  expect(screen.getByRole("option", { name: /Earlier backup · 1 KB/ })).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Backup to restore"), { target: { value: "snapshot" } });
  fireEvent.click(screen.getByRole("button", { name: "Replace library" }));
  expect(restored).not.toHaveBeenCalled();
  finish();
  await waitFor(() => expect(restored).toHaveBeenCalledOnce());
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("keeps focus inside Restore and announces an in-progress restore", async () => {
  const rects = vi.spyOn(HTMLElement.prototype, "getClientRects")
    .mockReturnValue([new DOMRect(0, 0, 1, 1)] as unknown as DOMRectList);
  try {
    const restore = vi.fn();
    vi.mocked(invoke).mockReset().mockResolvedValueOnce([
      { id: "snapshot", name: "Fixture", size: 1000 },
    ]).mockImplementationOnce(() => new Promise((_, reject) => { restore.mockImplementation(reject); }));
    const closeSettings = vi.fn();
    render(<Modal title="Settings" onCancel={closeSettings}>
      <DriveSettings status={connected} onChange={vi.fn()} />
    </Modal>);
    const opener = screen.getByRole("button", { name: "Backup history…" });
    opener.focus();
    fireEvent.click(opener);
    const picker = await screen.findByLabelText("Backup to restore");
    fireEvent.change(picker, { target: { value: "snapshot" } });
    const dialog = screen.getByRole("dialog", { name: "Restore a Drive backup" });
    expect(document.getElementById(dialog.getAttribute("aria-describedby")!)?.textContent)
      .toContain("replaces the links");
    const confirm = screen.getByRole("button", { name: "Replace library" });
    const cancel = screen.getByRole("button", { name: "Cancel" });
    confirm.focus();
    fireEvent.keyDown(confirm, { key: "Tab" });
    expect(document.activeElement).toBe(picker);
    fireEvent.keyDown(picker, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(confirm);
    fireEvent.click(confirm);
    expect(document.activeElement).toBe(confirm);
    expect(within(dialog).getByRole("status").textContent).toBe("Restoring backup…");
    fireEvent.click(cancel);
    fireEvent.click(confirm);
    expect(invoke).toHaveBeenCalledTimes(2);
    restore({ code: "invalid_backup", message: "This backup is invalid." });
    await screen.findByRole("alert");
    fireEvent(dialog, new Event("cancel", { bubbles: true, cancelable: true }));
    expect(closeSettings).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(opener);
  } finally {
    rects.mockRestore();
  }
});
