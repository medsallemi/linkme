import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { CapturePanel } from "./CapturePanel";
import {
  dismissCapturePanel,
  savePanelLink,
  takeCapturePanel,
  resizeCapturePanel,
} from "./capture";
import { listCategories, saveLink } from "./links";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./capture", () => ({
  dismissCapturePanel: vi.fn(),
  savePanelLink: vi.fn(),
  takeCapturePanel: vi.fn(),
  resizeCapturePanel: vi.fn(() => Promise.resolve()),
}));
vi.mock("./links", async (original) => ({
  ...(await original<typeof import("./links")>()),
  listCategories: vi.fn(),
  saveLink: vi.fn(),
}));

const url = "https://example.com/capture?keep=%2F#Exact";
const result = {
  created: true,
  link: {
    id: 9,
    revision: 0,
    url,
    categoryId: 1,
    categoryName: "Inbox",
    title: "",
    subtitle: "",
    caption: "",
  },
};

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(takeCapturePanel).mockResolvedValue({ kind: "prompt", url });
  vi.mocked(listCategories).mockResolvedValue([{ id: 1, name: "Inbox" }]);
  vi.mocked(dismissCapturePanel).mockResolvedValue();
  vi.mocked(invoke).mockResolvedValue(undefined);
  document.documentElement.dataset.popupPresented = "true";
});

afterEach(() => {
  delete document.documentElement.dataset.popupGeneration;
  delete document.documentElement.dataset.popupPresented;
  vi.unstubAllGlobals();
});

it("reports Capture readiness after the prefilled editor mounts and focuses Title", async () => {
  delete document.documentElement.dataset.popupPresented;
  document.documentElement.dataset.popupGeneration = "8";
  let focusedOnReady: Element | null = null;
  vi.mocked(invoke).mockImplementation(async () => {
    focusedOnReady = document.activeElement;
  });
  render(<CapturePanel />);
  expect(invoke).not.toHaveBeenCalled();
  const title = await screen.findByLabelText("Title");
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledExactlyOnceWith("popup_ready", {
      generation: 8,
    }),
  );
  expect(focusedOnReady).toBe(title);
  expect(resizeCapturePanel).not.toHaveBeenCalled();
});

it("prefills and focuses the floating editor, then cancels without saving", async () => {
  render(<CapturePanel />);
  const dialog = await screen.findByRole("dialog", { name: "Add link" });
  const input = within(dialog).getByLabelText("URL") as HTMLTextAreaElement;
  expect(input.value).toBe(url);
  expect(document.activeElement).toBe(
    within(dialog).getByLabelText("Title"),
  );
  fireEvent.change(within(dialog).getByLabelText("Title"), {
    target: { value: "Unsaved" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(dismissCapturePanel).toHaveBeenCalledOnce();
  expect(savePanelLink).not.toHaveBeenCalled();
});

it("closes the frameless prompt using its labeled close button without saving", async () => {
  render(<CapturePanel />);
  fireEvent.click(await screen.findByRole("button", { name: "Close capture" }));
  expect(dismissCapturePanel).toHaveBeenCalledOnce();
  expect(savePanelLink).not.toHaveBeenCalled();
});

it("leaves Escape in the editor to the dialog's cancel behavior", async () => {
  render(<CapturePanel />);
  const dialog = await screen.findByRole("dialog", { name: "Add link" });
  const input = within(dialog).getByLabelText("URL");
  expect(fireEvent.keyDown(input, { key: "Escape" })).toBe(true);
  expect(dismissCapturePanel).not.toHaveBeenCalled();
  fireEvent(dialog, new Event("cancel", { cancelable: true }));
  expect(dismissCapturePanel).toHaveBeenCalledOnce();
  expect(savePanelLink).not.toHaveBeenCalled();
});

it("saves through the capture-only command and closes after success", async () => {
  vi.mocked(savePanelLink).mockImplementation(async (_, shown) => {
    shown(true);
    return result;
  });
  render(<CapturePanel />);
  fireEvent.change(await screen.findByLabelText("Title"), { target: { value: "Captured title" } });
  const more = await screen.findByText("More details");
  expect(more.closest("details")?.open).toBe(false);
  fireEvent.click(more);
  fireEvent.change(screen.getByLabelText("Caption (optional)"), {
    target: { value: "Capture note" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Save link" }));
  await waitFor(() => expect(dismissCapturePanel).toHaveBeenCalledOnce());
  expect(savePanelLink).toHaveBeenCalledWith(
    expect.objectContaining({
      url,
      categoryId: 1,
      id: null,
      caption: "Capture note",
    }),
    expect.any(Function),
  );
  expect(saveLink).not.toHaveBeenCalled();
});

it("keeps the committed category visible until the native handoff, even on repeated submit", async () => {
  let show: (value: boolean) => void = () => {};
  vi.mocked(savePanelLink).mockImplementation(async (_, ready) => {
    show = ready;
    return { ...result, link: { ...result.link, categoryName: "Reading" } };
  });
  render(<CapturePanel />);
  fireEvent.change(await screen.findByLabelText("Title"), { target: { value: "Captured title" } });
  const save = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Save link",
  });
  fireEvent.click(save);
  expect(await screen.findByText("Saved to Reading.")).toBeTruthy();
  expect(save.disabled).toBe(true);
  fireEvent.submit(save.form!);
  expect(savePanelLink).toHaveBeenCalledTimes(1);
  expect(dismissCapturePanel).not.toHaveBeenCalled();
  await act(async () => show(true));
  await waitFor(() => expect(dismissCapturePanel).toHaveBeenCalledOnce());
});

it("shows duplicate and write error feedback without a successful handoff", async () => {
  vi.mocked(savePanelLink)
    .mockRejectedValueOnce({ code: "database", message: "Unable to save." })
    .mockResolvedValueOnce({ ...result, created: false });
  render(<CapturePanel />);
  fireEvent.change(await screen.findByLabelText("Title"), { target: { value: "Captured title" } });
  const save = await screen.findByRole("button", { name: "Save link" });
  fireEvent.click(save);
  await screen.findByText(/The local database is unavailable/);
  expect(screen.queryByText("Saved to Inbox.")).toBeNull();
  fireEvent.click(save);
  await screen.findByText("Already saved.");
  expect(dismissCapturePanel).not.toHaveBeenCalled();
});

it("shows committed Quick-save feedback without mounting the editor", async () => {
  vi.mocked(takeCapturePanel).mockResolvedValue({ kind: "saved", result });
  render(<CapturePanel />);
  expect(await screen.findByText("Saved to Inbox.")).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(savePanelLink).not.toHaveBeenCalled();
});

it("keeps Quick-save duplicate context readable until dismissal", async () => {
  vi.mocked(takeCapturePanel).mockResolvedValue({ kind: "saved", result: {
    created: false,
    link: { ...result.link, title: "Existing article", categoryName: "Reading", groupName: "Papers" },
  } });
  render(<CapturePanel />);
  expect((await screen.findByText("Already saved.")).closest("[role=status]")?.textContent)
    .toContain("Existing articleReading / Papers");
  expect(screen.queryByText(/^Saved to /)).toBeNull();
  expect(dismissCapturePanel).not.toHaveBeenCalled();
  fireEvent.keyDown(screen.getByRole("button", { name: "Dismiss" }), { key: "Escape" });
  expect(dismissCapturePanel).toHaveBeenCalledOnce();
});

it("focuses invalid-clipboard feedback so Escape can dismiss it", async () => {
  vi.mocked(takeCapturePanel).mockResolvedValue({
    kind: "error",
    error: { code: "invalid_url", message: "Copy one link." },
  });
  render(<CapturePanel />);
  await screen.findByRole("alert");
  const dismiss = screen.getByRole("button", { name: "Dismiss" });
  await waitFor(() => expect(document.activeElement).toBe(dismiss));
  fireEvent.keyDown(dismiss, { key: "Escape" });
  expect(dismissCapturePanel).toHaveBeenCalledOnce();
});

it("dismisses invalid-clipboard feedback with Escape when the page has focus", async () => {
  vi.mocked(takeCapturePanel).mockResolvedValue({
    kind: "error",
    error: { code: "invalid_url", message: "Copy one link." },
  });
  render(<CapturePanel />);
  await screen.findByRole("alert");
  const dismiss = screen.getByRole("button", { name: "Dismiss" });
  await waitFor(() => expect(document.activeElement).toBe(dismiss));
  dismiss.blur();
  expect(document.activeElement).toBe(document.body);

  fireEvent.keyDown(document.body, { key: "Escape", isComposing: true });
  expect(dismissCapturePanel).not.toHaveBeenCalled();
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(dismissCapturePanel).toHaveBeenCalledOnce();
  expect(savePanelLink).not.toHaveBeenCalled();
});

it("blocks a submit during animated cancellation and closes without an animation event", async () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
  render(<CapturePanel />);
  const save = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Save link",
  });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(save.disabled).toBe(true);
  fireEvent.submit(save.form!);
  expect(savePanelLink).not.toHaveBeenCalled();
  await waitFor(() => expect(dismissCapturePanel).toHaveBeenCalledOnce());
});

it("closes immediately and still rejects submit while native dismissal is pending in reduced motion", async () => {
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.mocked(dismissCapturePanel).mockImplementation(
    () => new Promise(() => {}),
  );
  render(<CapturePanel />);
  const save = await screen.findByRole<HTMLButtonElement>("button", {
    name: "Save link",
  });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(dismissCapturePanel).toHaveBeenCalledOnce();
  fireEvent.submit(save.form!);
  expect(savePanelLink).not.toHaveBeenCalled();
});

it("shows only the host in the chip and updates it as the URL is edited", async () => {
  render(<CapturePanel />);
  const field = await screen.findByLabelText("URL");
  expect(screen.getByText("example.com")).toBeTruthy();
  fireEvent.change(field, {
    target: { value: "https://example.org/private?key=value#part" },
  });
  expect(screen.getByText("example.org")).toBeTruthy();
  fireEvent.change(field, { target: { value: "file://server/path" } });
  expect(screen.queryByText("server")).toBeNull();
});

it("suppresses the scrollbar until the latest details resize has completed", async () => {
  const finish = new Map<boolean, () => void>();
  vi.mocked(resizeCapturePanel).mockImplementation(
    (expanded) => new Promise<void>((resolve) => finish.set(expanded, resolve)),
  );
  render(<CapturePanel />);
  const title = await screen.findByLabelText("Title");
  fireEvent.change(title, { target: { value: "Keep my title" } });
  const details = screen.getByText("More details").closest("details")!;
  const fields = details.closest(".editor-fields")!;
  details.open = true;
  fireEvent(details, new Event("toggle"));
  await waitFor(() => expect(resizeCapturePanel).toHaveBeenCalledWith(true));
  expect(fields.hasAttribute("data-resizing")).toBe(true);
  details.open = false;
  fireEvent(details, new Event("toggle"));
  expect(resizeCapturePanel).toHaveBeenCalledWith(false);
  await act(async () => finish.get(true)!());
  expect(fields.hasAttribute("data-resizing")).toBe(true);
  await act(async () => finish.get(false)!());
  expect(fields.hasAttribute("data-resizing")).toBe(false);
  expect((title as HTMLInputElement).value).toBe("Keep my title");
});

it("offers existing groups in the current capture category", async () => {
  vi.mocked(listCategories).mockResolvedValue([
    {id:1,name:"Inbox",groups:[{id:6,name:"Papers"}]}, {id:2,name:"Work"}
  ]);
  vi.mocked(savePanelLink).mockResolvedValue(result);
  render(<CapturePanel />);
  fireEvent.change(await screen.findByLabelText("Title"), { target: { value: "Captured title" } });
  const group = await screen.findByLabelText("Group (optional)");
  fireEvent.change(group, {target:{value:"6"}});
  fireEvent.click(screen.getByRole("button",{name:"Save link"}));
  await waitFor(() => expect(savePanelLink).toHaveBeenCalledWith(
    expect.objectContaining({categoryId:1,groupId:6}),expect.any(Function)
  ));
});
it("expands New category, focuses its name, and keeps the draft when returning to a group", async () => {
  vi.mocked(listCategories).mockResolvedValue([
    {id:1,name:"Inbox",groups:[{id:6,name:"Papers"}]},
    {id:2,name:"Work",groups:[{id:7,name:"Projects"}]},
  ]);
  render(<CapturePanel />);
  const category = await screen.findByLabelText("Category");
  const details = screen.getByText("More details").closest("details")!;
  const title = screen.getByLabelText<HTMLInputElement>("Title");
  fireEvent.change(title, {target:{value:"Keep this draft"}});
  fireEvent.change(screen.getByLabelText("Group (optional)"), {target:{value:"6"}});
  fireEvent.change(category, {target:{value:"__new__"}});
  expect(resizeCapturePanel).toHaveBeenCalledExactlyOnceWith(true);
  expect(details.open).toBe(true);
  expect(document.activeElement).toBe(screen.getByLabelText("New category name"));
  expect(screen.queryByLabelText("Group (optional)")).toBeNull();
  details.open = false;
  fireEvent(details, new Event("toggle"));
  expect(resizeCapturePanel).toHaveBeenCalledTimes(1);
  fireEvent.change(category, {target:{value:"2"}});
  expect(resizeCapturePanel).toHaveBeenLastCalledWith(false);
  const group = screen.getByLabelText<HTMLSelectElement>("Group (optional)");
  expect(group.value).toBe("");
  expect(within(group).getByRole("option", {name:"Projects"})).toBeTruthy();
  expect(within(group).queryByRole("option", {name:"Papers"})).toBeNull();
  expect(title.value).toBe("Keep this draft");
  expect(savePanelLink).not.toHaveBeenCalled();
});

it("rejects a whitespace-only capture title inline and submits after correction", async () => {
  vi.mocked(savePanelLink).mockResolvedValue(result);
  render(<CapturePanel />);
  const title = await screen.findByLabelText("Title");
  const save = await screen.findByRole("button", { name: "Save link" });
  fireEvent.change(title, {target: {value:"   "}});
  fireEvent.click(save);
  expect(savePanelLink).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(title);
  expect(title.getAttribute("aria-invalid")).toBe("true");
  expect(screen.getByRole("alert").textContent).toBe("Enter a title to save this link.");
  fireEvent.change(title, {target: {value:"Reading list"}});
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.click(save);
  await waitFor(() => expect(savePanelLink).toHaveBeenCalledWith(
    expect.objectContaining({title:"Reading list"}), expect.any(Function)
  ));
});
