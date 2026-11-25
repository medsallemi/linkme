import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { releaseRepository } from "./release-source.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const target = "x86_64-pc-windows-msvc";
const releaseRoot = path.join(root, "src-tauri/target/release");
const work = path.join(releaseRoot, "windows-setup-work");
const suffix = process.argv[2];
const option = process.argv[3];
if (process.argv.length > 4 || (option !== undefined && option !== "--require-drive-oauth") ||
    (suffix !== undefined && !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(suffix))) {
  throw new Error("Provide a lowercase stage suffix and optionally --require-drive-oauth.");
}
const desktopOAuthConfigured = Boolean(process.env.LINKME_DESKTOP_OAUTH_FILE);
if (option && !desktopOAuthConfigured) {
  throw new Error("Set LINKME_DESKTOP_OAUTH_FILE to the approved Google Desktop client export.");
}
const stage = path.join(releaseRoot, suffix ? `windows-setup-${suffix}` : "windows-setup");
const cliVersion = "2.12.6";
const sourceUrl = `https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v${cliVersion}/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`;
const templateHash = "d00089d3b1a2c4a30d7b6e098079100cb8f25e980f84d4515aca4597b95e967d";
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

if (process.platform !== "win32") throw new Error("Build the Windows x64 installer on Windows.");

const config = JSON.parse(await readFile(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
const appPackage = JSON.parse(await readFile(path.join(root, "package.json"), "utf8"));
const updateSource = releaseRepository(
  appPackage.repository?.url,
  process.env.LINKME_RELEASE_REPOSITORY,
);
const cliDir = path.join(root, "node_modules/@tauri-apps/cli");
const cli = JSON.parse(await readFile(path.join(cliDir, "package.json"), "utf8"));
if (cli.version !== cliVersion) {
  throw new Error("Review the installer template and its safety changes for this Tauri CLI version.");
}
await mkdir(work, { recursive: true });
const sourcePath = path.join(work, "tauri-installer.nsi");
let officialTemplate;
try {
  officialTemplate = await readFile(sourcePath);
} catch (error) {
  if (error.code !== "ENOENT") throw error;
  const response = await fetch(sourceUrl, { signal: AbortSignal.timeout(30000) });
  if (!response.ok) throw new Error(`Tauri template download failed (${response.status}).`);
  officialTemplate = Buffer.from(await response.arrayBuffer());
}
if (sha256(officialTemplate) !== templateHash) throw new Error("Tauri template SHA-256 mismatch.");
await writeFile(sourcePath, officialTemplate);
let template = officialTemplate.toString("utf8").replaceAll("\r\n", "\n");

function replaceOnce(before, after) {
  const index = template.indexOf(before);
  if (index < 0 || template.indexOf(before, index + before.length) >= 0) {
    throw new Error(`Unexpected Tauri template for ${before.split("\n")[0]}`);
  }
  template = template.slice(0, index) + after + template.slice(index + before.length);
}

function replaceBetween(first, last, after) {
  const start = template.indexOf(first);
  const end = template.indexOf(last, start + first.length);
  if (start < 0 || end < 0) throw new Error(`Missing Tauri template block ${first}`);
  replaceOnce(template.slice(start, end), after);
}

// Keep Tauri's MUI, runtime provisioning, shell integration and version resources.
// Upgrade in place, without invoking a previous uninstaller.
replaceOnce('VIAddVersionKey "ProductName" "${PRODUCTNAME}"',
  'VIAddVersionKey "ProductName" "${PRODUCTNAME}"\nVIAddVersionKey "CompanyName" "${MANUFACTURER}"');
replaceBetween("; 4. Custom page", "; 6. Start menu", "");
replaceOnce("Var WixMode\nVar OldMainBinaryName\n", "");
replaceBetween("  ; Abort downgrades", "FunctionEnd", [
  "  !insertmacro LinkmeInitializeSetup",
  "",
  '  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"',
  '  ${If} $R0 != ""',
  '    nsis_tauri_utils::SemverCompare "${VERSION}" $R0',
  '    Pop $R0',
  '    ${If} $R0 = -1',
  '      MessageBox MB_ICONSTOP "A newer version of Linkme is installed." /SD IDOK',
  '      SetErrorLevel 3',
  '      Abort',
  '    ${EndIf}',
  '  ${EndIf}',
  "",
].join("\n"));

replaceOnce('nsis_tauri_utils::RunAsUser "$INSTDIR\\${MAINBINARYNAME}.exe" ""',
  'nsis_tauri_utils::RunAsUser "$INSTDIR\\${MAINBINARYNAME}.exe" "--open-library"');
replaceOnce("!define MUI_STARTMENUPAGE_DEFAULTFOLDER \"${STARTMENUFOLDER}\"",
  "!define MUI_STARTMENUPAGE_DEFAULTFOLDER \"${STARTMENUFOLDER}\"\n" +
  "  !define MUI_STARTMENUPAGE_REGISTRY_ROOT HKCU\n" +
  '  !define MUI_STARTMENUPAGE_REGISTRY_KEY "${MANUPRODUCTKEY}"\n' +
  '  !define MUI_STARTMENUPAGE_REGISTRY_VALUENAME "StartMenuFolder"');
replaceOnce("  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive\n  !define MUI_STARTMENUPAGE_DEFAULTFOLDER",
  "  !define MUI_PAGE_CUSTOMFUNCTION_PRE LinkmeSkipExisting\n  !define MUI_STARTMENUPAGE_DEFAULTFOLDER");
replaceOnce("!insertmacro MUI_PAGE_INSTFILES",
  "!insertmacro LinkmeSetupFunctions\n!insertmacro MUI_PAGE_INSTFILES");
replaceOnce("Function un.onInit\n", "Function un.onInit\n  !insertmacro LinkmeLockSetup\n");
replaceBetween("  ; Create desktop shortcut for silent", "  !ifmacrodef NSIS_HOOK_POSTINSTALL", "");
replaceBetween("Var DeleteAppDataCheckbox\n", "!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive", "");
replaceBetween("  ; Removes the Autostart entry", "  !ifmacrodef NSIS_HOOK_POSTUNINSTALL", "");
replaceBetween("  ; Remove old main binary", "  ; Save current MAINBINARYNAME", "");

const skipShortcut = [
  "  ${If} $UpdateMode = 1",
  "  ${OrIf} $NoShortcutMode = 1",
  "    Return",
  "  ${EndIf}",
  "",
].join("\n");
replaceBetween("Function CreateOrUpdateStartMenuShortcut\n", '  !if "${STARTMENUFOLDER}"', "Function CreateOrUpdateStartMenuShortcut\n" + skipShortcut);
replaceBetween("Function CreateOrUpdateDesktopShortcut\n", '  CreateShortcut "$DESKTOP', "Function CreateOrUpdateDesktopShortcut\n" + skipShortcut);

const stockClose = '!insertmacro CheckIfAppIsRunning "$INSTDIR\\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"';
if (template.split(stockClose).length !== 3) throw new Error("Missing installer or uninstaller process check.");
template = template.replaceAll(stockClose, "!insertmacro LinkmeRequireStopped");
replaceOnce("Section WebView2\n", "Section -LinkmePreflight\n  !insertmacro LinkmeRequireStopped\nSectionEnd\n\nSection WebView2\n");

// Per-instance prerequisite path and quoted invocation. Treat an empty or zero version as missing.
replaceBetween("Section WebView2", "Section Install", template.slice(
  template.indexOf("Section WebView2"), template.indexOf("Section Install"),
).replaceAll('  ${If} $4 == ""', '  ${If} $4 == ""\n  ${OrIf} $4 == "0.0.0.0"')
  .replaceAll(String.raw`$TEMP\MicrosoftEdgeWebview2Setup.exe`, String.raw`$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe`)
  .replace('        Delete "$PLUGINSDIR\\MicrosoftEdgeWebview2Setup.exe"', "        InitPluginsDir")
  .replace('        ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1',
    '        ClearErrors\n        ExecWait \'"$6" ${WEBVIEW2INSTALLERARGS} /install\' $1\n        IfErrors 0 +2\n          Abort "$(webview2AbortError)"'));

const deleteMain = '  Delete "$INSTDIR\\${MAINBINARYNAME}.exe"';
replaceOnce(deleteMain, `  ClearErrors\n${deleteMain}\n  IfErrors 0 +2\n    Abort "Could not remove Linkme. Close it from the notification area, then retry."`);
const copyMain = '  File "${MAINBINARYSRCPATH}"';
replaceOnce(copyMain, `  ClearErrors\n${copyMain}\n  IfErrors 0 +2\n    Abort "Could not copy Linkme. Check disk space and access, then retry Setup."`);
if (/\b(?:CheckIfAppIsRunning|RmShutdown|FindWixInstall|OldMainBinaryName|WixMode|DeleteAppDataCheckbox)\b/.test(template)) {
  throw new Error("Unexpected legacy installer behavior after applying the Linkme policy.");
}

await writeFile(path.join(work, "linkme-installer.nsi"), template);

execFileSync(process.execPath, [
  path.join(cliDir, "tauri.js"), "build", "--ci", "--target", target,
  "--bundles", "nsis", "--config", "src-tauri/tauri.nsis.conf.json",
], {
  cwd: root,
  stdio: "inherit",
  env: { ...process.env, LINKME_RELEASE_REPOSITORY: updateSource ?? "" },
});

const release = path.join(root, "src-tauri/target", target, "release");
const installer = `Linkme_${config.version}_x64-setup.exe`;
const inputs = [
  [path.join(release, "bundle/nsis", installer), installer],
  [path.join(release, "linkme.exe"), "linkme.exe"],
];
await mkdir(stage, { recursive: true });
const artifacts = [];
for (const [source, name] of inputs) {
  const staged = path.join(stage, name);
  await copyFile(source, staged);
  const bytes = await readFile(staged);
  const hash = sha256(bytes);
  if (hash !== sha256(await readFile(source))) throw new Error(`Staged checksum mismatch for ${name}`);
  artifacts.push({ name, bytes: bytes.length, sha256: hash });
}
await writeFile(path.join(stage, "SHA256SUMS.txt"), artifacts.map((item) =>
  `${item.sha256}  ${item.name}\n`).join(""));
await writeFile(path.join(stage, "ARTIFACTS.json"), JSON.stringify({
  product: config.productName, identifier: config.identifier, version: config.version,
  target, tauriCli: cliVersion, format: "NSIS", releaseRepository: updateSource, desktopOAuthConfigured, artifacts,
}, null, 2) + "\n");
console.log(`Staged ${artifacts.length} release artifacts in ${stage}`);
