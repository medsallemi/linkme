# Google Drive backup and restore

## Connect and back up

In the Library, open **Capture settings → Cloud backup → Connect Google Drive**. Sign in and grant access in your browser. After Linkme saves the connection, choose **Open Linkme** in the browser or return to the Library manually. The official installer includes Google sign-in configuration.

Connection alone does not upload your library. Choose **Back up now** or set a schedule. Backups are SQLite snapshots of the links, notes, categories and groups on this device. They are stored in **Linkme Backups** in your Drive. Backups have a 5 MB limit and contain readable library data.

| Schedule | Copies retained by Linkme |
| --- | --- |
| Off | Up to 7 manual copies |
| Daily | 7 |
| Weekly | 4 |
| Monthly | 12 |

After a successful upload and a complete listing, Linkme moves older eligible backups from this installation to Drive Trash. It leaves shared, unrelated and other installations' files alone.

## Restore a snapshot

Open **Backup history**, select a backup, and choose **Replace library**. This replaces the local links, categories and groups after Linkme saves a recovery copy. The app then reloads the Library. Your device preferences, Google account and backup schedule stay connected.

If Google rejects an expired grant, use **Reconnect** with the same account. **Disconnect** clears the local connection and schedule. Existing backups remain in Drive. To revoke the grant at Google, remove Linkme in your [Google Account connections](https://myaccount.google.com/connections). [Privacy and permissions](PRIVACY.md)
