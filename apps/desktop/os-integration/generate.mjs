// Generates the "Convert with AriadShift" file-manager registrations from formats.json.
//
// formats.json is derived from ariad-core and checked by the ariad-desktop file_open tests.
// Usage: node apps/desktop/os-integration/generate.mjs [--check]
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { env as runtimeEnvironment } from "node:process";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tauriDir = join(here, "../src-tauri");

export const label = "Convert with AriadShift";
export const verbKey = "AriadShift.Convert";
export const workflowName = `${label}.workflow`;

// Uniform Type Identifiers by registry format id. A format without an entry fails generation,
// so a new input format cannot ship without a macOS type.
const utis = {
  markdown: "net.daringfireball.markdown",
  html: "public.html",
  docx: "org.openxmlformats.wordprocessingml.document",
  epub: "org.idpf.epub-container",
  pdf: "com.adobe.pdf",
  png: "public.png",
  pptx: "org.openxmlformats.presentationml.presentation",
  xlsx: "org.openxmlformats.spreadsheetml.sheet",
  odt: "org.oasis-open.opendocument.text",
  ods: "org.oasis-open.opendocument.spreadsheet",
  odp: "org.oasis-open.opendocument.presentation",
  doc: "com.microsoft.word.doc",
  xls: "com.microsoft.excel.xls",
  ppt: "com.microsoft.powerpoint.ppt",
  rtf: "public.rtf",
  jpeg: "public.jpeg",
  webp: "org.webmproject.webp",
  tiff: "public.tiff",
};

/** Tauri applies platform config and the aggregated CLI override using JSON Merge Patch. */
export function mergeConfig(base, patch) {
  if (!patch || typeof patch !== "object" || Array.isArray(patch)) return patch;
  const result = base && typeof base === "object" && !Array.isArray(base) ? { ...base } : {};
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) delete result[key];
    else result[key] = mergeConfig(result[key], value);
  }
  return result;
}

export function loadContext(overrides = {}) {
  const { formats } = JSON.parse(readFileSync(join(here, "formats.json"), "utf8"));
  const root = JSON.parse(readFileSync(join(tauriDir, "tauri.conf.json"), "utf8"));
  const windows = JSON.parse(readFileSync(join(tauriDir, "tauri.windows.conf.json"), "utf8"));
  const config = mergeConfig(mergeConfig(root, windows), overrides);
  const cargo = readFileSync(join(tauriDir, "Cargo.toml"), "utf8");
  const binary = /^default-run\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
  if (!binary) throw new Error("Cargo.toml must name the desktop binary in default-run");
  for (const format of formats) {
    if (!utis[format.id]) throw new Error(`No macOS type identifier for format ${format.id}`);
  }
  const resources = config.bundle.resources;
  if (!resources || Array.isArray(resources) || typeof resources !== "object") {
    throw new Error("Windows MSI resources require explicit source-to-filename mappings");
  }
  const resourceFiles = Object.entries(resources)
    .sort((a, b) => a[1].localeCompare(b[1]))
    .map(([source, name]) => {
      if (typeof name !== "string" || !name || /[\\/]/.test(name) || /[*?{}[\]]/.test(source)) {
        throw new Error("Windows MSI resources require individual files with root filenames");
      }
      if (!statSync(resolve(tauriDir, source)).isFile()) {
        throw new Error("Windows MSI resources must be files");
      }
      return {
        source: relative(join(tauriDir, "windows"), resolve(tauriDir, source)).replaceAll(
          "\\",
          "/",
        ),
        name,
      };
    });
  if (config.bundle.windows.bundleVcRuntime) {
    throw new Error("The per-user MSI cannot package implicit VC runtime resources");
  }
  if (config.bundle.windows.wix.enableElevatedUpdateTask) {
    throw new Error("The per-user MSI cannot install an elevated update task");
  }
  return { formats, identifier: config.identifier, binary, resourceFiles };
}

const xml = (text) => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
const attribute = (text) => xml(text).replaceAll('"', "&quot;");
const plistStrings = (values, indent) =>
  values.map((value) => `${indent}<string>${xml(value)}</string>`).join("\n");
const plist = (body) =>
  `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
${body}
</dict>
</plist>
`;

const typeIdentifiers = (formats) => [...new Set(formats.map((format) => utis[format.id]))];
const extensions = (formats) => formats.flatMap((format) => format.extensions);
const mediaTypes = (formats) => [...new Set(formats.map((format) => format.media_type))];

/** The app's Info.plist additions: document types for Finder "Open With" and drops on the Dock. */
function appInfoPlist({ formats }) {
  const markdown = formats.find((format) => format.id === "markdown");
  return plist(`	<key>CFBundleIconName</key>
	<string>AriadShift</string>
	<key>CFBundleDocumentTypes</key>
	<array>
		<dict>
			<key>CFBundleTypeName</key>
			<string>Convertible document</string>
			<key>CFBundleTypeRole</key>
			<string>Viewer</string>
			<key>LSHandlerRank</key>
			<string>Alternate</string>
			<key>LSItemContentTypes</key>
			<array>
${plistStrings(typeIdentifiers(formats), "\t\t\t\t")}
			</array>
		</dict>
	</array>
	<key>UTImportedTypeDeclarations</key>
	<array>
		<dict>
			<key>UTTypeIdentifier</key>
			<string>${utis.markdown}</string>
			<key>UTTypeDescription</key>
			<string>Markdown document</string>
			<key>UTTypeConformsTo</key>
			<array>
				<string>public.plain-text</string>
			</array>
			<key>UTTypeTagSpecification</key>
			<dict>
				<key>public.filename-extension</key>
				<array>
${plistStrings(markdown.extensions, "\t\t\t\t\t")}
				</array>
				<key>public.mime-type</key>
				<array>
					<string>${markdown.media_type}</string>
				</array>
			</dict>
		</dict>
	</array>`);
}

/** The Quick Action bundle's Info.plist: the Services entry Finder shows. */
function workflowInfoPlist({ formats, identifier }) {
  return plist(`	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleIdentifier</key>
	<string>${identifier}.quick-action</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>${label}</string>
	<key>CFBundlePackageType</key>
	<string>BNDL</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>${label}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
${plistStrings(typeIdentifiers(formats), "\t\t\t\t")}
			</array>
		</dict>
	</array>`);
}

/** The Quick Action's Automator document: one "Run Shell Script" step with files as arguments. */
function workflowDocument({ identifier }) {
  // Finder passes absolute paths; Launch Services delivers them to the app as an "open
  // documents" event, which reaches the running instance instead of starting a second one.
  const command = `/usr/bin/open -b ${identifier} "$@"\n`;
  return plist(`	<key>AMApplicationBuild</key>
	<string>521</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.path</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>${xml(command)}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/sh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>5D1C2B9E-7A43-4C1B-9E0A-3F6A2D8B4C11</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
				</array>
				<key>OutputUUID</key>
				<string>8B2E4F60-1C3D-4A5B-8E7F-9A0B1C2D3E22</string>
				<key>UUID</key>
				<string>A3F1D2C4-5B6E-4F70-8192-A3B4C5D6E733</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
			</dict>
			<key>isViewVisible</key>
			<true/>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>serviceApplicationBundleID</key>
		<string>com.apple.finder</string>
		<key>serviceApplicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<false/>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>`);
}

const verbPath = (extension) =>
  `Software\\Classes\\SystemFileAssociations\\.${extension}\\shell\\${verbKey}`;

/** NSIS installer hooks: per-user Explorer verbs that never change the default app. */
function nsisHooks({ formats }) {
  // biome-ignore lint/suspicious/noTemplateCurlyInString: NSIS expands this installer constant.
  const exe = '$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\"';
  const write = extensions(formats)
    .map((extension) => {
      const key = verbPath(extension);
      return [
        `  WriteRegStr SHCTX "${key}" "MUIVerb" "${label}"`,
        `  WriteRegStr SHCTX "${key}" "Icon" "${exe},0"`,
        `  WriteRegStr SHCTX "${key}" "MultiSelectModel" "Player"`,
        `  WriteRegStr SHCTX "${key}\\command" "" "${exe} $\\"%1$\\""`,
      ].join("\n");
    })
    .join("\n");
  const remove = extensions(formats)
    .map((extension) => `  DeleteRegKey SHCTX "${verbPath(extension)}"`)
    .join("\n");
  return `; Generated by apps/desktop/os-integration/generate.mjs. Do not edit.
; "${label}" Explorer verbs for supported extensions. They live under
; SystemFileAssociations, so they add a menu entry without taking over the default app.
; SHCTX follows the installer's install mode, which is per user (currentUser).

!macro ARIADSHIFT_SHELL_CHANGED
  ; SHCNE_ASSOCCHANGED tells Explorer to reload its menus.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTINSTALL
${write}
  !insertmacro ARIADSHIFT_SHELL_CHANGED
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
${remove}
  !insertmacro ARIADSHIFT_SHELL_CHANGED
!macroend
`;
}

/** WiX fragment for the MSI: the same verbs, removed with the component on uninstall. */
function wixFragment({ formats, resourceFiles }) {
  const keys = extensions(formats)
    .map(
      (
        extension,
      ) => `        <RegistryKey Root="HKCU" Key="${verbPath(extension)}" ForceDeleteOnUninstall="yes">
          <RegistryValue Name="MUIVerb" Type="string" Value="${label}" />
          <RegistryValue Name="Icon" Type="string" Value="&quot;[!Path]&quot;,0" />
          <RegistryValue Name="MultiSelectModel" Type="string" Value="Player" />
          <RegistryKey Key="command">
            <RegistryValue Type="string" Value="&quot;[!Path]&quot; &quot;%1&quot;" />
          </RegistryKey>
        </RegistryKey>`,
    )
    .join("\n");
  const hash = createHash("sha256").update(JSON.stringify(resourceFiles)).digest("hex");
  const resourceGuid = `${hash.slice(0, 8)}-${hash.slice(8, 12)}-${hash.slice(12, 16)}-${hash.slice(16, 20)}-${hash.slice(20, 32)}`;
  const resourceXml = resourceFiles
    .map(
      ({ source, name }, index) =>
        `        <File Id="BundleResource${index}" Source="$(sys.SOURCEFILEDIR)${attribute(source.replaceAll("/", "\\"))}" Name="${attribute(name)}" KeyPath="no" />`,
    )
    .join("\n");
  return `<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by apps/desktop/os-integration/generate.mjs. Do not edit. -->
<!-- "${label}" Explorer verbs; [!Path] is the main executable from Tauri's main.wxs. -->
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Fragment>
    <DirectoryRef Id="INSTALLDIR">
      <Component Id="ExplorerConvertVerbs" Guid="{6F3B2A1C-9D4E-4B7A-8C21-5E0F7A9B3D46}">
        <RegistryValue Root="HKCU" Key="Software\\AriadShift\\OsIntegration" Name="ExplorerVerbs" Type="integer" Value="1" KeyPath="yes" />
${keys}
      </Component>
      <Component Id="BundleResources" Guid="{${resourceGuid}}">
        <RegistryValue Root="HKCU" Key="Software\\AriadShift\\OsIntegration" Name="BundleResources" Type="integer" Value="1" KeyPath="yes" />
${resourceXml}
      </Component>
    </DirectoryRef>
    <ComponentGroup Id="PerUserBundleResources">
      <ComponentRef Id="BundleResources" />
    </ComponentGroup>
  </Fragment>
</Wix>
`;
}

/** Handlebars template for the packaged Linux .desktop file (Tauri fills the variables). */
function desktopTemplate({ formats }) {
  return `[Desktop Entry]
Categories={{categories}}
{{#if comment}}
Comment={{comment}}
{{/if}}
Exec={{exec}} %U
StartupWMClass={{exec}}
Icon={{icon}}
Name={{name}}
Terminal=false
Type=Application
MimeType={{#if mime_type}}{{mime_type}};{{/if}}${mediaTypes(formats).join(";")};
Actions=convert;

[Desktop Action convert]
Name=${label}
Exec={{exec}} %U
`;
}

/** Dolphin service menu (KDE Frameworks 5 and 6 read the same file). */
function dolphinServiceMenu({ formats, binary }) {
  return `# Generated by apps/desktop/os-integration/generate.mjs. Do not edit.
[Desktop Entry]
Type=Service
X-KDE-ServiceTypes=KonqPopupMenu/Plugin
MimeType=${mediaTypes(formats).join(";")};
Actions=convertWithAriadShift;
X-KDE-Priority=TopLevel

[Desktop Action convertWithAriadShift]
Name=${label}
Icon=${binary}
Exec=${binary} -- %F
`;
}

/** Nautilus script. Nautilus only reads scripts from the user's home, so users copy it there. */
function nautilusScript({ binary }) {
  return `#!/bin/sh
# Generated by apps/desktop/os-integration/generate.mjs. Do not edit.
# "${label}" for Nautilus (GNOME Files). Install it for the current user:
#   install -Dm755 /usr/share/ariadshift/nautilus/convert-with-ariadshift \\
#     "$HOME/.local/share/nautilus/scripts/${label}"
# Nautilus runs scripts in the current folder and passes the selected names as arguments.
exec ${binary} -- "$@"
`;
}

/** Every generated file, keyed by its path relative to apps/desktop. */
export function render(context = loadContext()) {
  const workflow = `os-integration/macos/${workflowName}/Contents`;
  return {
    "src-tauri/Info.plist": appInfoPlist(context),
    [`${workflow}/Info.plist`]: workflowInfoPlist(context),
    [`${workflow}/document.wflow`]: workflowDocument(context),
    "src-tauri/windows/os-integration.nsh": nsisHooks(context),
    "src-tauri/windows/os-integration.wxs": wixFragment(context),
    "os-integration/linux/main.desktop.hbs": desktopTemplate(context),
    "os-integration/linux/dolphin/ariadshift-convert.desktop": dolphinServiceMenu(context),
    "os-integration/linux/nautilus/convert-with-ariadshift": nautilusScript(context),
  };
}

/** Returns the generated paths whose committed content differs from the rendered content. */
export function stale(files = render()) {
  return Object.entries(files)
    .filter(([path, content]) => {
      try {
        return readFileSync(join(here, "..", path), "utf8") !== content;
      } catch {
        return true;
      }
    })
    .map(([path]) => path);
}

/** Keep the pre-bundle check bound to the build output Tauri actually uses. */
export function windowsTauriArgs(args, host, environment, hasCargoConfig = false) {
  if (!args.some((arg) => ["build", "bundle"].includes(arg))) return { args, windows: false };
  const separator = args.indexOf("--");
  const options = separator < 0 ? args : args.slice(0, separator);
  const targetAt = options.findIndex((arg) => arg === "--target" || arg === "-t");
  const target =
    options.find((arg) => arg.startsWith("--target="))?.slice(9) ??
    options
      .find((arg) => arg.startsWith("-t") && !arg.startsWith("--") && arg.length > 2)
      ?.slice(2)
      .replace(/^=/, "") ??
    (targetAt >= 0 ? options[targetAt + 1] : host);
  if (!target?.includes("-windows-")) return { args, windows: false };
  if (!target.endsWith("-msvc")) throw new Error("Per-user Windows installers require MSVC");
  const unsupported = ["--profile", "--target-dir"];
  const cargoOptions = separator < 0 ? [] : args.slice(separator + 1);
  if (
    args.some((arg) => unsupported.some((flag) => arg === flag || arg.startsWith(`${flag}=`))) ||
    cargoOptions.some(
      (arg) =>
        /^-[qv]*[rmC]/.test(arg) ||
        ["--config", "--manifest-path", "--target", "-t", "--release", "-r"].some(
          (flag) => arg === flag || arg.startsWith(`${flag}=`),
        ),
    ) ||
    environment.CARGO_BUILD_TARGET ||
    environment.TAURI_CONFIG
  ) {
    throw new Error(
      "Per-user Windows packaging requires standard debug/release outputs and CLI --config overrides; custom Cargo output options and inherited TAURI_CONFIG are unsupported",
    );
  }
  const explicit =
    targetAt >= 0 ||
    options.some(
      (arg) =>
        arg.startsWith("--target=") ||
        (arg.startsWith("-t") && !arg.startsWith("--") && arg.length > 2),
    );
  if (!explicit && hasCargoConfig) {
    throw new Error("Pass an explicit MSVC --target when Cargo configuration files exist");
  }
  return { args, windows: true, output: explicit ? "target" : "native" };
}

function cargoConfigPresent() {
  let directory = tauriDir;
  while (true) {
    if (["config", "config.toml"].some((name) => existsSync(join(directory, ".cargo", name))))
      return true;
    const parent = dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }
  const cargoHome = runtimeEnvironment.CARGO_HOME ?? join(homedir(), ".cargo");
  return ["config", "config.toml"].some((name) => existsSync(join(cargoHome, name)));
}

export function checkWindowsOutput(directory, binary) {
  if (!statSync(join(directory, `${binary}.exe`)).isFile()) {
    throw new Error("The current Windows executable is missing before bundling");
  }
  if (readdirSync(directory).some((name) => name.toLowerCase().endsWith(".dll"))) {
    throw new Error("The per-user MSI requires explicit HKCU components for output DLLs");
  }
}

function checkWindowsBundle() {
  const triple = runtimeEnvironment.TAURI_ENV_TARGET_TRIPLE;
  if (runtimeEnvironment.ARIAD_MSI_CANONICAL_BUILD !== "1" || !triple?.endsWith("-windows-msvc")) {
    throw new Error("Build Windows installers through the desktop package's tauri command");
  }
  const context = loadContext(JSON.parse(runtimeEnvironment.TAURI_CONFIG ?? "{}"));
  const outdated = stale(render(context));
  if (outdated.length)
    throw new Error(
      "Effective installer configuration differs from generated OS integration; regenerate it before bundling",
    );
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
      cwd: tauriDir,
      encoding: "utf8",
    }),
  );
  const profile = runtimeEnvironment.TAURI_ENV_DEBUG === "true" ? "debug" : "release";
  const output = runtimeEnvironment.ARIAD_MSI_OUTPUT_KIND;
  if (output !== "native" && output !== "target") throw new Error("Missing Windows output binding");
  const directory =
    output === "target"
      ? join(metadata.target_directory, triple, profile)
      : join(metadata.target_directory, profile);
  checkWindowsOutput(directory, context.binary);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === "--tauri") {
    const args = process.argv.slice(3);
    const version = args.some((arg) => ["build", "bundle"].includes(arg))
      ? execFileSync("rustc", ["-vV"], { encoding: "utf8" })
      : "";
    const host = /^host: (.+)$/m.exec(version)?.[1];
    const invocation = windowsTauriArgs(args, host, runtimeEnvironment, cargoConfigPresent());
    const environment = Object.assign({}, runtimeEnvironment);
    delete environment.TAURI_ENV_DEBUG;
    delete environment.ARIAD_MSI_CANONICAL_BUILD;
    delete environment.ARIAD_MSI_OUTPUT_KIND;
    if (invocation.windows) {
      environment.ARIAD_MSI_CANONICAL_BUILD = "1";
      environment.ARIAD_MSI_OUTPUT_KIND = invocation.output;
    }
    const child = spawnSync(
      process.execPath,
      [join(here, "../scripts/tauri.mjs"), ...invocation.args],
      { env: environment, stdio: "inherit" },
    );
    if (child.error) throw child.error;
    process.exit(child.status ?? 1);
  }
  if (process.argv[2] === "--check-windows-resources") {
    checkWindowsBundle();
    process.exit(0);
  }
  const files = render(loadContext(JSON.parse(runtimeEnvironment.TAURI_CONFIG ?? "{}")));
  if (process.argv.includes("--check")) {
    const outdated = stale(files);
    if (outdated.length) {
      console.error(`Stale OS integration files; run generate.mjs:\n  ${outdated.join("\n  ")}`);
      process.exit(1);
    }
  } else {
    for (const [path, content] of Object.entries(files)) {
      const target = join(here, "..", path);
      mkdirSync(dirname(target), { recursive: true });
      writeFileSync(target, content, {
        mode: path.endsWith("convert-with-ariadshift") ? 0o755 : 0o644,
      });
    }
  }
}
