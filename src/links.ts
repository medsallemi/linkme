import { invoke } from "@tauri-apps/api/core";

export interface Link {
  id: number;
  url: string;
  title: string;
  subtitle: string;
  caption: string;
  categoryId: number;
  groupId?: number | null;
  categoryName: string;
  groupName?: string | null;
  categoryIcon?: string;
  categoryColor?: number | null;
  revision: number;
}
export interface Category {
  id: number;
  name: string;
  icon?: string;
  color?: number | null;
  pinned?: boolean;
  groups?: { id: number; name: string }[];
}
export interface Cursor {
  rank: number;
  id: number;
}
export interface LinkPage {
  links: Link[];
  next: Cursor | null;
}
export interface LinkDraft {
  id: number | null;
  revision: number | null;
  url: string;
  title: string;
  subtitle: string;
  caption: string;
  categoryId: number;
  groupId?: number | null;
  newCategory: string | null;
  newCategoryIcon?: string;
}
export interface SaveResult {
  link: Link;
  created: boolean;
}
export function listLinks(
  query = "",
  before: Cursor | null = null,
): Promise<LinkPage> {
  return invoke("list_links", { query, before });
}
export function updateCategory(
  id: number,
  name: string,
  icon: string,
  color: number | null,
  pinned: boolean,
): Promise<void> {
  return invoke("update_category", { id, name, icon, color, pinned });
}
export function listCategories(): Promise<Category[]> {
  return invoke("list_categories");
}
export function saveLink(input: LinkDraft): Promise<SaveResult> {
  return invoke("save_link", { input });
}
export function deleteLink(id: number, revision: number): Promise<void> {
  return invoke("delete_link", { id, revision });
}
export function copyLink(id: number): Promise<void> {
  return invoke("copy_link", { id });
}
export function openLink(id: number): Promise<void> {
  return invoke("open_link", { id });
}
export function errorMessage(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof error.message === "string" &&
    [
      "invalid_url",
      "invalid_backup",
      "invalid_metadata",
      "invalid_category",
      "invalid_query",
      "duplicate_url",
      "conflict",
      "not_found",
      "native_action",
      "clipboard",
      "invalid_shortcut",
      "shortcut_conflict",
      "capture_busy",
    ].includes(String(error.code))
  )
    return error.message;
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === "database"
  )
    return "The local database is unavailable. Check disk space and access, then retry.";
  return "The result could not be confirmed. Keep your draft and reload the library before retrying.";
}

export function orderLink(id: number, target: number, before: boolean): Promise<void> {
  return invoke("order_link", { id, target, before });
}
