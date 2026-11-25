# Using Linkme

## Save and find links

Copy an HTTP or HTTPS URL and press **Ctrl+Shift+L**. Prompt mode lets you enter a title and choose a category and group. **Save** commits the link. Press **Esc** to cancel. Existing links are kept when you submit a duplicate.

Enable **Quick-save** in Capture settings to save straight to Inbox. Press **Ctrl+Shift+K** to search from another app. Search covers titles, URLs, notes, categories and groups. Use the arrow keys to choose a result and press Enter to open it.

## Manage your library

Open the Library from the Linkme tray icon. Create or edit categories, organize links into groups, and pin important categories. Use **Organize** to reorder items. Row actions let you open, copy, edit, move or delete a link. Delete requires confirmation.

Closing the Library hides it while the tray stays active. Choose **Quit** from the tray to exit completely. On a first install, Linkme starts in the tray at Windows sign-in. Turn that off in Capture settings.

## Updates and backups

Download updates from [GitHub Releases](https://github.com/medsallemi/linkme/releases). When configured, the app can notify you about newer compatible stable releases. It opens the release page for you to download and run Setup. Quit the running app before installing an update.

Uninstall leaves the local library and account settings in your Windows profile. Use [Drive backup](DRIVE_BACKUP.md) or save a copy of the local library before changing devices.

## Known limitations

- Windows 11 x64 is the release target. The installer requires WebView2 and is unsigned. Device policy can block installation or a WebView2 download.
- Website previews and icons depend on the public site's availability, supported response types and Linkme's network restrictions. A text card or category icon is the fallback.
- Optional Drive backup uses snapshots of one device's library. Very large libraries may exceed the current 5 MB snapshot limit.
- Acrylic and motion are subject to Windows and WebView2 behavior. Linkme provides solid and reduced-motion options. Narrator, mixed-DPI setups and broader device coverage have limited verification.
