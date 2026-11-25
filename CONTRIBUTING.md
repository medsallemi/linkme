# Contributing to Linkme

Start with [GitHub Issues](https://github.com/medsallemi/linkme/issues). Discuss new dependencies, schema changes, operating system permissions or new network requests before implementing them.

1. Read [building on Windows](docs/BUILD.md) and [architecture](docs/ARCHITECTURE.md).
2. Branch from `main` and keep the change focused. Add or update a regression test when behavior changes.
3. Run the checks in the build guide. Use a disposable Windows profile and synthetic links for native tests.
4. Describe the user-visible change, tests you ran and what still needs a Windows check in your pull request.

For bugs, include the Linkme version, Windows build, and steps to reproduce. For visual or focus issues, include the WebView2 version, display scale, and appearance setting if relevant. Keep private links, databases, OAuth files, tokens and unredacted screenshots out of issues and commits.

Report vulnerabilities privately using the [security policy](SECURITY.md).
