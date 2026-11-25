# Privacy and permissions

Linkme stores your library and settings in SQLite in the current user's Windows application-data folder. Local use requires no Linkme account. Linkme reads the clipboard when you invoke Capture and has no telemetry.

| Feature | When Linkme makes a request |
| --- | --- |
| Website icons | Visible saved rows can request a favicon from the public website, independently of hover previews. |
| Link previews | If enabled, a hover or focus can send the public URL's path and query to the website and contact an eligible thumbnail host. Cookies, credentials and fragments are excluded. Turn previews off in Settings. |
| Google Drive | Sign-in requests basic profile identity and access to files used with Linkme. If you start or schedule a backup, the app sends a library snapshot to your Drive. |
| Update checks | Configured builds can request public release metadata from GitHub. Requests carry no library data or Google credentials. Turn automatic checks off in Settings. |

Drive backup requests `openid`, profile, and `drive.file`. The app uses your account identifier to recognize the linked account and can display the name and photo. Windows DPAPI protects the local Google session. The local SQLite library and backup copies are readable data.

Disconnect removes the local Google session and schedule. Drive copies remain until removed in Drive, and revocation is available from your Google Account settings. See [backup and restore](DRIVE_BACKUP.md) for retention. Uninstall preserves local data. Reset's confirmation lists what it removes and retains.

[Full privacy policy](https://linkme.bacontrack.tn/privacy) · [Terms](https://linkme.bacontrack.tn/terms) · [Maintainer contact](mailto:machekelsallemi@gmail.com)
