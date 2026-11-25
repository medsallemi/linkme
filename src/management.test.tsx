import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import App from "./App";

vi.mock("./links", () => ({
  listLinks: vi.fn().mockResolvedValue({ links: [], next: null }),
  saveLink: vi.fn(),
  listCategories: vi.fn().mockResolvedValue([{ id: 1, name: "Inbox" }]),
  deleteLink: vi.fn(),
  copyLink: vi.fn(),
  openLink: vi.fn(),
}));
afterEach(cleanup);
beforeEach(() => vi.clearAllMocks());

it("opens a keyboard-accessible link editor from the library", async () => {
  render(<App />);
  const add = await screen.findByRole("button", { name: "Add link" });
  await new Promise((r) => setTimeout(r, 0));
  fireEvent.click(add);
  expect(await screen.findByRole("dialog", { name: "Add link" })).toBeTruthy();
  expect(screen.getByLabelText("Category")).toBeTruthy();
  expect(screen.getByLabelText("Title (optional)")).toBeTruthy();
});
