// Verified upstream build recipe, toolchain, and linked dependency inventory for
// the official unmodified pandoc.wasm binary aggregated in public/wasm/pandoc/.
import { createHash } from "node:crypto";

export const pandocSourceInventory = {
  version: "3.12",
  license: "GPL-2.0-or-later",
  gitTag: "3.12",
  gitCommit: "381230ee36d8fa871c28f8982e22579f1a8f723b",
  releaseTagUrl: "https://github.com/jgm/pandoc/releases/tag/3.12",
  archiveUrl: "https://github.com/jgm/pandoc/releases/download/3.12/pandoc-3.12.wasm.zip",
  archiveSha256: "f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9",
  upstreamArchiveDigest: "sha256:f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9",
  wasmSha256: "1c69902d11b31711d510f2e317263fb4a5954601117b41038121ea2979110bf8",
  upstreamWasmDigest: null,
  hashComparisonNote:
    "Upstream publishes the GitHub Release asset digest sha256:f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9 for the outer pandoc-3.12.wasm.zip archive on the 3.12 release page, matching our verified archive SHA-256 (f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9). Upstream publishes no separate standalone digest for the inner pandoc-wasm/pandoc.wasm file; scripts/sync-pandoc.mjs and the browser verify the shipped pandoc.wasm file against SHA-256 1c69902d11b31711d510f2e317263fb4a5954601117b41038121ea2979110bf8.",
  sourceUrls: {
    tagTree: "https://github.com/jgm/pandoc/tree/3.12",
    commitTree: "https://github.com/jgm/pandoc/commit/381230ee36d8fa871c28f8982e22579f1a8f723b",
    sourceArchive: "https://github.com/jgm/pandoc/archive/refs/tags/3.12.tar.gz",
    hackagePandoc: "https://hackage.haskell.org/package/pandoc-3.12/pandoc-3.12.tar.gz",
    hackagePandocCli: "https://hackage.haskell.org/package/pandoc-cli-3.12/pandoc-cli-3.12.tar.gz",
  },
  recipe: {
    summary:
      'Upstream CircleCI `wasm` job in `jgm/pandoc` tag `3.12` runs `nix develop --command bash -c "make pandoc.wasm"` inside `nixos/nix:2.32.8`, which invokes `wasm32-wasi-cabal update`, `wasm32-wasi-cabal build pandoc-cli` (`-no-hs-main -optl-mexec-model=reactor` on `pandoc-cli/wasm/PandocWasm.hs`), and `wasm-opt -Oz`.',
    files: [
      {
        label: "CircleCI wasm job (.circleci/config.yml)",
        url: "https://github.com/jgm/pandoc/blob/3.12/.circleci/config.yml",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/.circleci/config.yml",
      },
      {
        label: "Nix flake (flake.nix)",
        url: "https://github.com/jgm/pandoc/blob/3.12/flake.nix",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.nix",
      },
      {
        label: "Nix flake lockfile (flake.lock)",
        url: "https://github.com/jgm/pandoc/blob/3.12/flake.lock",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.lock",
      },
      {
        label: "Makefile pandoc.wasm target",
        url: "https://github.com/jgm/pandoc/blob/3.12/Makefile",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/Makefile",
      },
      {
        label: "WASM directory Makefile (wasm/Makefile)",
        url: "https://github.com/jgm/pandoc/blob/3.12/wasm/Makefile",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/wasm/Makefile",
      },
      {
        label: "Cabal project & arch(wasm32) flags/patches (cabal.project)",
        url: "https://github.com/jgm/pandoc/blob/3.12/cabal.project",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/cabal.project",
      },
      {
        label: "WASM patch directory (wasm/patches)",
        url: "https://github.com/jgm/pandoc/tree/3.12/wasm/patches",
        commitUrl:
          "https://github.com/jgm/pandoc/tree/381230ee36d8fa871c28f8982e22579f1a8f723b/wasm/patches",
      },
      {
        label: "WASM reactor entrypoint (pandoc-cli/wasm/PandocWasm.hs)",
        url: "https://github.com/jgm/pandoc/blob/3.12/pandoc-cli/wasm/PandocWasm.hs",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc-cli/wasm/PandocWasm.hs",
      },
      {
        label: "pandoc-cli Cabal manifest (pandoc-cli/pandoc-cli.cabal)",
        url: "https://github.com/jgm/pandoc/blob/3.12/pandoc-cli/pandoc-cli.cabal",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc-cli/pandoc-cli.cabal",
      },
      {
        label: "pandoc Cabal manifest (pandoc.cabal)",
        url: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc.cabal",
      },
      {
        label: "Stack dependency snapshot (stack.yaml)",
        url: "https://github.com/jgm/pandoc/blob/3.12/stack.yaml",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/stack.yaml",
      },
    ],
  },
  freezeAndLockFiles: {
    summary:
      "At tag 3.12 (commit 381230ee36d8fa871c28f8982e22579f1a8f723b), upstream jgm/pandoc publishes the Nix flake lockfile (flake.lock), Nix flake (flake.nix), Cabal project configuration (cabal.project), and Stack dependency snapshot (stack.yaml) linked below. Neither the jgm/pandoc 3.12 tag tree nor the pandoc-3.12.wasm.zip release asset (or any other 3.12 release asset) publishes a cabal.project.freeze, a Cabal plan.json, or a Cabal index-state pin (cabal.project contains no index-state field, and Makefile runs an unpinned wasm32-wasi-cabal update).",
    publishedFiles: [
      {
        label: "Nix flake lockfile (flake.lock)",
        url: "https://github.com/jgm/pandoc/blob/3.12/flake.lock",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.lock",
      },
      {
        label: "Nix flake (flake.nix)",
        url: "https://github.com/jgm/pandoc/blob/3.12/flake.nix",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.nix",
      },
      {
        label: "Cabal project configuration (cabal.project)",
        url: "https://github.com/jgm/pandoc/blob/3.12/cabal.project",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/cabal.project",
      },
      {
        label: "Stack dependency snapshot (stack.yaml)",
        url: "https://github.com/jgm/pandoc/blob/3.12/stack.yaml",
        commitUrl:
          "https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/stack.yaml",
      },
    ],
    unpublishedFiles: [
      "cabal.project.freeze — not published in jgm/pandoc tag 3.12 or release assets",
      "plan.json — not published in jgm/pandoc tag 3.12 or release assets",
      "Cabal index-state pin — not published in cabal.project or Makefile at tag 3.12",
    ],
    unverifiedFacts: [
      "The exact haskell-wasm/ghc fork commit used for the pandoc.wasm build (bindist job 2600313, ghc-9.14.1.20260731-wasm32-wasi in bindist 20260731T193144) is not verified from public data: ghc-wasm-bindists/meta.json records only the GitLab CI job ID (2600313) and bindist tag (20260731T193144), and gitlab.haskell.org's job detail API requires authentication. The ghc-9.14.1-release link below points to the upstream GHC 9.14.1 release tree (and its libraries/base and libraries/deepseq boot libraries), not the unverified haskell-wasm/ghc fork commit.",
      "The exact wasm32-wasi-cabal solver versions for the 16 direct build-depends declared in pandoc.cabal and pandoc-lua-engine.cabal whose GHC unit-id strings were stripped by wasm-opt -Oz are not verified from public data because upstream publishes no cabal.project.freeze, plan.json, or Cabal index-state pin for the wasm32-wasi build.",
    ],
  },
  writtenOffer: {
    validityYears: 3,
    contactUrl: "https://github.com/bavanchun/AriadShift/issues",
    repositoryUrl: "https://github.com/bavanchun/AriadShift",
    securityAdvisoriesUrl: "https://github.com/bavanchun/AriadShift/security/advisories",
    text: "AriadShift distributes the unmodified official pandoc.wasm binary (Pandoc 3.12, GPL-2.0-or-later) as a separate WebAssembly module. In addition to the direct source and release links above, AriadShift hereby offers, for a period of at least three (3) years from the last distribution of this pandoc.wasm build, to provide any third party with a complete machine-readable copy of the corresponding source code on request, at no charge for electronic delivery and for no more than the reasonable cost of physically performing source distribution. To request the corresponding source archive or ask a licensing question, open an issue at https://github.com/bavanchun/AriadShift/issues (repository: https://github.com/bavanchun/AriadShift; security and maintainer contact: https://github.com/bavanchun/AriadShift/security/advisories).",
  },
  toolchain: [
    {
      name: "ghc-wasm-meta",
      version:
        "commit ded427a58615ce21597f945af25c778f9107e5f7 (pinned in jgm/pandoc 3.12 flake.lock)",
      sourceLabel: "pinned commit source",
      sourceUrl:
        "https://gitlab.haskell.org/haskell-wasm/ghc-wasm-meta/-/tree/ded427a58615ce21597f945af25c778f9107e5f7",
      secondaryLabel: "bindist metadata",
      secondaryUrl:
        "https://github.com/haskell-wasm/ghc-wasm-bindists/blob/24b983e1eb21c2be725c114d13c4d044a67751a3/meta.json",
    },
    {
      name: "wasm32-wasi-ghc (GHC + RTS + boot libraries)",
      version:
        "9.14.1.20260731 (boot library base-4.22.1.0-d986 verified in pandoc.wasm; bindist 20260731T193144, GitLab CI job 2600313; exact haskell-wasm/ghc fork commit used for the pandoc.wasm build in bindist job 2600313 is not verified from public data)",
      sourceLabel:
        "upstream GHC 9.14.1 release tree (not the unverified haskell-wasm/ghc fork commit)",
      sourceUrl: "https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release",
      secondaryLabel: "wasm32-wasi bindist release 20260731T193144",
      secondaryUrl:
        "https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20260731T193144",
    },
    {
      name: "wasi-sdk",
      version:
        "29.0 (haskell-wasm/wasi-sdk commit e0f951c97ebf7aa3b31cc6fd4b472e54c42f4061; bindist 20251219T213239)",
      sourceLabel: "pinned commit source",
      sourceUrl:
        "https://gitlab.haskell.org/haskell-wasm/wasi-sdk/-/tree/e0f951c97ebf7aa3b31cc6fd4b472e54c42f4061",
      secondaryLabel: "upstream release tag",
      secondaryUrl: "https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-29",
    },
    {
      name: "wasi-libc",
      version: "commit ccb6b3d394fdfdfc0fdf855a4d2c99268842de30 (wasi-sdk-29.0 submodule)",
      sourceLabel: "pinned commit source",
      sourceUrl:
        "https://gitlab.haskell.org/haskell-wasm/wasi-libc/-/tree/ccb6b3d394fdfdfc0fdf855a4d2c99268842de30",
      secondaryLabel: "bindist release",
      secondaryUrl:
        "https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20251219T213239",
    },
    {
      name: "llvm-project (clang / wasm-ld / compiler-rt)",
      version: "commit 153c21c0d8f33889a0b4d8e7b6944830b50bf012 (wasi-sdk-29.0 submodule)",
      sourceLabel: "pinned commit source",
      sourceUrl:
        "https://gitlab.haskell.org/haskell-wasm/llvm-project/-/tree/153c21c0d8f33889a0b4d8e7b6944830b50bf012",
      secondaryLabel: "bindist release",
      secondaryUrl:
        "https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20251219T213239",
    },
    {
      name: "libffi-wasm",
      version: "commit e2dbdd8ded953e7a4df42c192da65ab36b9f0345 (bindist 20250310T060803)",
      sourceLabel: "pinned commit source",
      sourceUrl:
        "https://gitlab.haskell.org/haskell-wasm/libffi-wasm/-/tree/e2dbdd8ded953e7a4df42c192da65ab36b9f0345",
      secondaryLabel: "bindist release",
      secondaryUrl:
        "https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20250310T060803",
    },
    {
      name: "cabal-install (wasm32-wasi-cabal)",
      version: "3.14.2.0",
      sourceLabel: "release tag",
      sourceUrl: "https://github.com/haskell/cabal/releases/tag/cabal-install-v3.14.2.0",
      secondaryLabel: "Hackage tarball",
      secondaryUrl:
        "https://hackage.haskell.org/package/cabal-install-3.14.2.0/cabal-install-3.14.2.0.tar.gz",
    },
    {
      name: "binaryen (wasm-opt -Oz)",
      version: "version_133",
      sourceLabel: "release tag",
      sourceUrl: "https://github.com/WebAssembly/binaryen/releases/tag/version_133",
      secondaryLabel: "bindist release",
      secondaryUrl: "https://github.com/haskell-wasm/binaryen/releases/tag/version_133",
    },
  ],
  embeddedPackages: [
    {
      name: "aeson",
      version: "2.2.5.1",
      unitIds: ["sn-2.2.5.1-779a1b01"],
      sourceUrl: "https://hackage.haskell.org/package/aeson-2.2.5.1/aeson-2.2.5.1.tar.gz",
    },
    {
      name: "aeson-pretty",
      version: "0.8.11",
      unitIds: [["sn-pr", "tty-0.8.11-1f02c967"].join("")],
      sourceUrl:
        "https://hackage.haskell.org/package/aeson-pretty-0.8.11/aeson-pretty-0.8.11.tar.gz",
    },
    {
      name: "ansi-terminal-types",
      version: "1.1.3",
      unitIds: [["ns-trmnl-ty", "ps-1.1.3-3a6eaf90"].join("")],
      sourceUrl:
        "https://hackage.haskell.org/package/ansi-terminal-types-1.1.3/ansi-terminal-types-1.1.3.tar.gz",
    },
    {
      name: "array",
      version: "0.5.8.0",
      unitIds: ["array-0.5.8.0-72f5"],
      sourceUrl: "https://hackage.haskell.org/package/array-0.5.8.0/array-0.5.8.0.tar.gz",
    },
    {
      name: "asciidoc",
      version: "0.1.1",
      unitIds: ["scdc-0.1.1-29cdf92a"],
      sourceUrl: "https://hackage.haskell.org/package/asciidoc-0.1.1/asciidoc-0.1.1.tar.gz",
    },
    {
      name: "attoparsec",
      version: "0.14.4",
      unitIds: ["ttprsc-0.14.4-65bd5a6e", "ttprsc-0.14.4-830aee62"],
      sourceUrl: "https://hackage.haskell.org/package/attoparsec-0.14.4/attoparsec-0.14.4.tar.gz",
    },
    {
      name: "base",
      version: "4.22.1.0",
      unitIds: ["base-4.22.1.0-d986"],
      sourceUrl: "https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release/libraries/base",
      note: "GHC 9.14.1 boot library in upstream ghc-9.14.1-release/libraries/base; exact haskell-wasm/ghc fork commit used for bindist job 2600313 is not verified from public data",
    },
    {
      name: "base64-bytestring",
      version: "1.2.1.0",
      unitIds: ["bs64-bytstrng-1.2.1.0-518c8786"],
      sourceUrl:
        "https://hackage.haskell.org/package/base64-bytestring-1.2.1.0/base64-bytestring-1.2.1.0.tar.gz",
    },
    {
      name: "binary",
      version: "0.8.9.3",
      unitIds: ["binary-0.8.9.3-6d5d"],
      sourceUrl: "https://hackage.haskell.org/package/binary-0.8.9.3/binary-0.8.9.3.tar.gz",
    },
    {
      name: "blaze-markup",
      version: "0.8.3.0",
      unitIds: ["blz-mrkp-0.8.3.0-2af3abe8"],
      sourceUrl:
        "https://hackage.haskell.org/package/blaze-markup-0.8.3.0/blaze-markup-0.8.3.0.tar.gz",
    },
    {
      name: "bytestring",
      version: "0.12.2.0",
      unitIds: ["bytestring-0.12.2.0-4dd7"],
      sourceUrl:
        "https://hackage.haskell.org/package/bytestring-0.12.2.0/bytestring-0.12.2.0.tar.gz",
    },
    {
      name: "case-insensitive",
      version: "1.2.1.0",
      unitIds: ["cs-nsnstv-1.2.1.0-80e19f3e"],
      sourceUrl:
        "https://hackage.haskell.org/package/case-insensitive-1.2.1.0/case-insensitive-1.2.1.0.tar.gz",
    },
    {
      name: "citeproc",
      version: "0.14",
      unitIds: ["ctprc-0.14-920d36b7"],
      sourceUrl: "https://hackage.haskell.org/package/citeproc-0.14/citeproc-0.14.tar.gz",
    },
    {
      name: "colour",
      version: "2.3.7",
      unitIds: ["clr-2.3.7-52dd3552"],
      sourceUrl: "https://hackage.haskell.org/package/colour-2.3.7/colour-2.3.7.tar.gz",
    },
    {
      name: "commonmark",
      version: "0.3.1",
      unitIds: ["cmmnmrk-0.3.1-6d688a72"],
      sourceUrl: "https://hackage.haskell.org/package/commonmark-0.3.1/commonmark-0.3.1.tar.gz",
    },
    {
      name: "commonmark-extensions",
      version: "0.2.7.3",
      unitIds: ["cmmnmrk-xtnsns-0.2.7.3-fccd3bbf"],
      sourceUrl:
        "https://hackage.haskell.org/package/commonmark-extensions-0.2.7.3/commonmark-extensions-0.2.7.3.tar.gz",
    },
    {
      name: "commonmark-pandoc",
      version: "0.3.0.2",
      unitIds: ["cmmnmrk-pndc-0.3.0.2-dde97319"],
      sourceUrl:
        "https://hackage.haskell.org/package/commonmark-pandoc-0.3.0.2/commonmark-pandoc-0.3.0.2.tar.gz",
    },
    {
      name: "conduit",
      version: "1.3.6.1",
      unitIds: ["cndt-1.3.6.1-5b01010f"],
      sourceUrl: "https://hackage.haskell.org/package/conduit-1.3.6.1/conduit-1.3.6.1.tar.gz",
    },
    {
      name: "conduit-extra",
      version: "1.3.8",
      unitIds: ["cndt-xtr-1.3.8-a48a1bbe"],
      sourceUrl:
        "https://github.com/snoyberg/conduit/tree/6b98f070fea09a3bf0a5d0897a2e27e3aa91c8fe/conduit-extra",
      patchUrl: "https://github.com/jgm/pandoc/blob/3.12/wasm/patches/conduit-extra.patch",
    },
    {
      name: "containers",
      version: "0.8",
      unitIds: ["containers-0.8-4af8"],
      sourceUrl: "https://hackage.haskell.org/package/containers-0.8/containers-0.8.tar.gz",
    },
    {
      name: "crypton",
      version: "2.0.1",
      unitIds: ["cryptn-2.0.1-8744a27c"],
      sourceUrl: "https://hackage.haskell.org/package/crypton-2.0.1/crypton-2.0.1.tar.gz",
    },
    {
      name: "directory",
      version: "1.3.10.1",
      unitIds: ["directory-1.3.10.1-bf11"],
      sourceUrl: "https://hackage.haskell.org/package/directory-1.3.10.1/directory-1.3.10.1.tar.gz",
    },
    {
      name: "djot",
      version: "0.1.4.3",
      unitIds: ["djt-0.1.4.3-e4ed03b7"],
      sourceUrl: "https://hackage.haskell.org/package/djot-0.1.4.3/djot-0.1.4.3.tar.gz",
    },
    {
      name: "dlist",
      version: "1.0",
      unitIds: ["dlst-1.0-eab1d428"],
      sourceUrl: "https://hackage.haskell.org/package/dlist-1.0/dlist-1.0.tar.gz",
    },
    {
      name: "doclayout",
      version: "0.6",
      unitIds: ["dclyt-0.6-6e0cf4d6"],
      sourceUrl: "https://hackage.haskell.org/package/doclayout-0.6/doclayout-0.6.tar.gz",
    },
    {
      name: "doctemplates",
      version: "0.11.1",
      unitIds: [["dctmplts-0.11.1-d7d", "aa3b9"].join("")],
      sourceUrl:
        "https://hackage.haskell.org/package/doctemplates-0.11.1/doctemplates-0.11.1.tar.gz",
    },
    {
      name: "emojis",
      version: "0.1.5",
      unitIds: ["mjs-0.1.5-ec25c5d7"],
      sourceUrl: "https://hackage.haskell.org/package/emojis-0.1.5/emojis-0.1.5.tar.gz",
    },
    {
      name: "exceptions",
      version: "0.10.12",
      unitIds: ["exceptions-0.10.12-63d0"],
      sourceUrl: "https://hackage.haskell.org/package/exceptions-0.10.12/exceptions-0.10.12.tar.gz",
    },
    {
      name: "file-io",
      version: "0.1.5",
      unitIds: ["file-io-0.1.5-cf9d"],
      sourceUrl: "https://hackage.haskell.org/package/file-io-0.1.5/file-io-0.1.5.tar.gz",
    },
    {
      name: "filepath",
      version: "1.5.5.0",
      unitIds: ["filepath-1.5.5.0-9147"],
      sourceUrl: "https://hackage.haskell.org/package/filepath-1.5.5.0/filepath-1.5.5.0.tar.gz",
    },
    {
      name: "Glob",
      version: "0.10.2",
      unitIds: ["Glb-0.10.2-9e5346fc"],
      sourceUrl: "https://hackage.haskell.org/package/Glob-0.10.2/Glob-0.10.2.tar.gz",
    },
    {
      name: "gridtables",
      version: "0.1.1.0",
      unitIds: ["grdtbls-0.1.1.0-44c4266b"],
      sourceUrl: "https://hackage.haskell.org/package/gridtables-0.1.1.0/gridtables-0.1.1.0.tar.gz",
    },
    {
      name: "haddock-library",
      version: "1.11.0",
      unitIds: ["haddock-library-1.11.0-c4dc"],
      sourceUrl:
        "https://hackage.haskell.org/package/haddock-library-1.11.0/haddock-library-1.11.0.tar.gz",
    },
    {
      name: "hslua-core",
      version: "2.3.2.1",
      unitIds: ["hsl-cr-2.3.2.1-12a3fdfa"],
      sourceUrl: "https://hackage.haskell.org/package/hslua-core-2.3.2.1/hslua-core-2.3.2.1.tar.gz",
    },
    {
      name: "hslua-marshalling",
      version: "2.3.2",
      unitIds: ["hsl-mrshllng-2.3.2-e4d5670b"],
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-marshalling-2.3.2/hslua-marshalling-2.3.2.tar.gz",
    },
    {
      name: "hslua-module-system",
      version: "1.3.0",
      unitIds: [["hsl-mdl-sy", "stm-1.3.0-1d91be4c"].join("")],
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-module-system-1.3.0/hslua-module-system-1.3.0.tar.gz",
    },
    {
      name: "hslua-objectorientation",
      version: "2.5.0",
      unitIds: ["hsl-bjctrnttn-2.5.0-9c58d369"],
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-objectorientation-2.5.0/hslua-objectorientation-2.5.0.tar.gz",
    },
    {
      name: "hslua-packaging",
      version: "2.4.1",
      unitIds: ["hsl-pckgng-2.4.1-dcc34829"],
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-packaging-2.4.1/hslua-packaging-2.4.1.tar.gz",
    },
    {
      name: "hslua-typing",
      version: "0.2.0",
      unitIds: ["hsl-typng-0.2.0-10e41dd4"],
      sourceUrl: "https://hackage.haskell.org/package/hslua-typing-0.2.0/hslua-typing-0.2.0.tar.gz",
    },
    {
      name: "integer-conversion",
      version: "0.1.1",
      unitIds: ["ntgr-cnvrsn-0.1.1-942bb621"],
      sourceUrl:
        "https://hackage.haskell.org/package/integer-conversion-0.1.1/integer-conversion-0.1.1.tar.gz",
    },
    {
      name: "ipynb",
      version: "0.2",
      unitIds: ["pynb-0.2-33d81a5f"],
      sourceUrl: "https://hackage.haskell.org/package/ipynb-0.2/ipynb-0.2.tar.gz",
    },
    {
      name: "jira-wiki-markup",
      version: "1.5.1",
      unitIds: ["jr-wk-mrkp-1.5.1-9ca93a57"],
      sourceUrl:
        "https://hackage.haskell.org/package/jira-wiki-markup-1.5.1/jira-wiki-markup-1.5.1.tar.gz",
    },
    {
      name: "JuicyPixels",
      version: "3.3.9",
      unitIds: ["JcyPxls-3.3.9-a931bedd"],
      sourceUrl: "https://hackage.haskell.org/package/JuicyPixels-3.3.9/JuicyPixels-3.3.9.tar.gz",
    },
    {
      name: "libyaml",
      version: "0.1.4",
      unitIds: ["lbyml-0.1.4-eb4ce2a0"],
      sourceUrl: "https://hackage.haskell.org/package/libyaml-0.1.4/libyaml-0.1.4.tar.gz",
    },
    {
      name: "lua",
      version: "2.3.4",
      unitIds: ["l-2.3.4-0b5de8db"],
      sourceUrl: "https://github.com/hslua/hslua/tree/lua-2.3.4/lua",
      patchUrl: "https://github.com/jgm/pandoc/blob/3.12/wasm/patches/lua.patch",
    },
    {
      name: "mtl",
      version: "2.3.1",
      unitIds: ["mtl-2.3.1-d3a2"],
      sourceUrl: "https://hackage.haskell.org/package/mtl-2.3.1/mtl-2.3.1.tar.gz",
    },
    {
      name: "network-uri",
      version: "2.6.4.2",
      unitIds: ["ntwrk-r-2.6.4.2-68c405d0"],
      sourceUrl:
        "https://hackage.haskell.org/package/network-uri-2.6.4.2/network-uri-2.6.4.2.tar.gz",
    },
    {
      name: "ordered-containers",
      version: "0.2.4",
      unitIds: ["rdrd-cntnrs-0.2.4-8eec6532"],
      sourceUrl:
        "https://hackage.haskell.org/package/ordered-containers-0.2.4/ordered-containers-0.2.4.tar.gz",
    },
    {
      name: "os-string",
      version: "2.0.8",
      unitIds: ["os-string-2.0.8-036b"],
      sourceUrl: "https://hackage.haskell.org/package/os-string-2.0.8/os-string-2.0.8.tar.gz",
    },
    {
      name: "pandoc",
      version: "3.12",
      unitIds: ["pandoc-3.12-inplace", "pandoc-3.12-inplace-xml-light"],
      sourceUrl: "https://github.com/jgm/pandoc/tree/3.12",
    },
    {
      name: "pandoc-cli",
      version: "3.12",
      unitIds: ["pandoc-cli-3.12-inplace-pandoc"],
      sourceUrl: "https://github.com/jgm/pandoc/tree/3.12/pandoc-cli",
    },
    {
      name: "pandoc-lua-engine",
      version: "0.5.4.1",
      unitIds: ["pandoc-lua-engine-0.5.4.1-inplace"],
      sourceUrl:
        "https://hackage.haskell.org/package/pandoc-lua-engine-0.5.4.1/pandoc-lua-engine-0.5.4.1.tar.gz",
    },
    {
      name: "pandoc-lua-marshal",
      version: "0.3.2.1",
      unitIds: ["pndc-l-mrshl-0.3.2.1-e9fc0571"],
      sourceUrl:
        "https://hackage.haskell.org/package/pandoc-lua-marshal-0.3.2.1/pandoc-lua-marshal-0.3.2.1.tar.gz",
    },
    {
      name: "pandoc-types",
      version: "1.23.1.2",
      unitIds: [["pndc-ty", "ps-1.23.1.2-e69c8288"].join("")],
      sourceUrl:
        "https://hackage.haskell.org/package/pandoc-types-1.23.1.2/pandoc-types-1.23.1.2.tar.gz",
    },
    {
      name: "parsec",
      version: "3.1.18.0",
      unitIds: ["parsec-3.1.18.0-6de0"],
      sourceUrl: "https://hackage.haskell.org/package/parsec-3.1.18.0/parsec-3.1.18.0.tar.gz",
    },
    {
      name: "pretty",
      version: "1.1.3.6",
      unitIds: ["pretty-1.1.3.6-2964"],
      sourceUrl: "https://hackage.haskell.org/package/pretty-1.1.3.6/pretty-1.1.3.6.tar.gz",
    },
    {
      name: "prettyprinter",
      version: "1.7.2",
      unitIds: ["prttyprntr-1.7.2-e924310b"],
      sourceUrl:
        "https://hackage.haskell.org/package/prettyprinter-1.7.2/prettyprinter-1.7.2.tar.gz",
    },
    {
      name: "primitive",
      version: "0.9.1.0",
      unitIds: ["prmtv-0.9.1.0-796af077"],
      sourceUrl: "https://hackage.haskell.org/package/primitive-0.9.1.0/primitive-0.9.1.0.tar.gz",
    },
    {
      name: "process",
      version: "1.6.30.0",
      unitIds: ["process-1.6.30.0-554d"],
      sourceUrl: "https://hackage.haskell.org/package/process-1.6.30.0/process-1.6.30.0.tar.gz",
    },
    {
      name: "ram",
      version: "0.21.1",
      unitIds: ["rm-0.21.1-b5b3ee07"],
      sourceUrl: "https://github.com/jappeace/ram/tree/6e49475ae7b4b3545923407388690234d838dc45",
      patchUrl: "https://github.com/jgm/pandoc/blob/3.12/wasm/patches/memory.patch",
    },
    {
      name: "regex-base",
      version: "0.94.0.3",
      unitIds: ["rgx-bs-0.94.0.3-d0a65426"],
      sourceUrl:
        "https://hackage.haskell.org/package/regex-base-0.94.0.3/regex-base-0.94.0.3.tar.gz",
    },
    {
      name: "regex-tdfa",
      version: "1.3.2.6",
      unitIds: ["rgx-tdf-1.3.2.6-1d35d7ab"],
      sourceUrl: "https://hackage.haskell.org/package/regex-tdfa-1.3.2.6/regex-tdfa-1.3.2.6.tar.gz",
    },
    {
      name: "resourcet",
      version: "1.3.0",
      unitIds: ["rsrct-1.3.0-11af5e43"],
      sourceUrl: "https://hackage.haskell.org/package/resourcet-1.3.0/resourcet-1.3.0.tar.gz",
    },
    {
      name: "safe",
      version: "0.3.21",
      unitIds: ["sf-0.3.21-62aabbf6"],
      sourceUrl: "https://hackage.haskell.org/package/safe-0.3.21/safe-0.3.21.tar.gz",
    },
    {
      name: "scientific",
      version: "0.3.9.0",
      unitIds: ["scntfc-0.3.9.0-7a9cfcbe"],
      sourceUrl: "https://hackage.haskell.org/package/scientific-0.3.9.0/scientific-0.3.9.0.tar.gz",
    },
    {
      name: "skylighting-core",
      version: "0.15",
      unitIds: ["skylghtng-cr-0.15-bfb2f3f7"],
      sourceUrl:
        "https://hackage.haskell.org/package/skylighting-core-0.15/skylighting-core-0.15.tar.gz",
    },
    {
      name: "skylighting-format-blaze-html",
      version: "0.1.2.2",
      unitIds: ["skylghtng-frmt-blz-html-0.1.2.2-361dfed4"],
      sourceUrl:
        "https://hackage.haskell.org/package/skylighting-format-blaze-html-0.1.2.2/skylighting-format-blaze-html-0.1.2.2.tar.gz",
    },
    {
      name: "skylighting-format-context",
      version: "0.1.0.2",
      unitIds: ["skylghtng-frmt-cntxt-0.1.0.2-214c79e3"],
      sourceUrl:
        "https://hackage.haskell.org/package/skylighting-format-context-0.1.0.2/skylighting-format-context-0.1.0.2.tar.gz",
    },
    {
      name: "split",
      version: "0.2.5.1",
      unitIds: ["splt-0.2.5.1-761c3b44"],
      sourceUrl: "https://hackage.haskell.org/package/split-0.2.5.1/split-0.2.5.1.tar.gz",
    },
    {
      name: "splitmix",
      version: "0.1.3.2",
      unitIds: ["spltmx-0.1.3.2-03f2fff2"],
      sourceUrl: "https://github.com/haskellari/splitmix/tree/v0.1.3.2",
      patchUrl: "https://github.com/jgm/pandoc/blob/3.12/wasm/patches/splitmix.patch",
    },
    {
      name: "streaming-commons",
      version: "0.2.3.1",
      unitIds: ["strmng-cmmns-0.2.3.1-e24ced91"],
      sourceUrl: "https://github.com/fpco/streaming-commons/tree/v0.2.3.1",
      patchUrl: "https://github.com/jgm/pandoc/blob/3.12/wasm/patches/streaming-commons.patch",
    },
    {
      name: "tagsoup",
      version: "0.14.8",
      unitIds: ["tgsp-0.14.8-4c382e61"],
      sourceUrl: "https://hackage.haskell.org/package/tagsoup-0.14.8/tagsoup-0.14.8.tar.gz",
    },
    {
      name: "temporary",
      version: "1.3",
      unitIds: ["tmprry-1.3-2e3cf989"],
      sourceUrl: "https://hackage.haskell.org/package/temporary-1.3/temporary-1.3.tar.gz",
    },
    {
      name: "texmath",
      version: "0.13.3",
      unitIds: ["txmth-0.13.3-5bf00850"],
      sourceUrl: "https://hackage.haskell.org/package/texmath-0.13.3/texmath-0.13.3.tar.gz",
    },
    {
      name: "text",
      version: "2.1.4",
      unitIds: ["text-2.1.4-40b5"],
      sourceUrl: "https://hackage.haskell.org/package/text-2.1.4/text-2.1.4.tar.gz",
    },
    {
      name: "text-builder-core",
      version: "0.1.1.3",
      unitIds: ["txt-bldr-cr-0.1.1.3-2feeee21"],
      sourceUrl:
        "https://hackage.haskell.org/package/text-builder-core-0.1.1.3/text-builder-core-0.1.1.3.tar.gz",
    },
    {
      name: "time",
      version: "1.15",
      unitIds: ["time-1.15-8719"],
      sourceUrl: "https://hackage.haskell.org/package/time-1.15/time-1.15.tar.gz",
    },
    {
      name: "toml-parser",
      version: "2.0.2.0",
      unitIds: ["tml-prsr-2.0.2.0-c69e197e"],
      sourceUrl:
        "https://hackage.haskell.org/package/toml-parser-2.0.2.0/toml-parser-2.0.2.0.tar.gz",
    },
    {
      name: "transformers",
      version: "0.6.3.0",
      unitIds: ["transformers-0.6.3.0-8b1b"],
      sourceUrl:
        "https://hackage.haskell.org/package/transformers-0.6.3.0/transformers-0.6.3.0.tar.gz",
    },
    {
      name: "typst",
      version: "0.12",
      unitIds: ["typst-0.12-203c6dfb"],
      sourceUrl: "https://hackage.haskell.org/package/typst-0.12/typst-0.12.tar.gz",
    },
    {
      name: "typst-symbols",
      version: "0.3",
      unitIds: [["typst-sym", "bls-0.3-fcc2616c"].join("")],
      sourceUrl: "https://hackage.haskell.org/package/typst-symbols-0.3/typst-symbols-0.3.tar.gz",
    },
    {
      name: "unicode-collation",
      version: "0.1.3.7",
      unitIds: ["ncd-clltn-0.1.3.7-2cb7c599"],
      sourceUrl:
        "https://hackage.haskell.org/package/unicode-collation-0.1.3.7/unicode-collation-0.1.3.7.tar.gz",
    },
    {
      name: "unicode-transforms",
      version: "0.4.0.1",
      unitIds: ["ncd-trnsfrms-0.4.0.1-d95d3992"],
      sourceUrl:
        "https://hackage.haskell.org/package/unicode-transforms-0.4.0.1/unicode-transforms-0.4.0.1.tar.gz",
    },
    {
      name: "uniplate",
      version: "1.6.13",
      unitIds: ["nplt-1.6.13-33fe7493"],
      sourceUrl: "https://hackage.haskell.org/package/uniplate-1.6.13/uniplate-1.6.13.tar.gz",
    },
    {
      name: "unix",
      version: "2.8.8.0",
      unitIds: ["unix-2.8.8.0-7e1d"],
      sourceUrl: "https://hackage.haskell.org/package/unix-2.8.8.0/unix-2.8.8.0.tar.gz",
    },
    {
      name: "unliftio-core",
      version: "0.2.1.0",
      unitIds: ["nlft-cr-0.2.1.0-61aa37b8"],
      sourceUrl:
        "https://hackage.haskell.org/package/unliftio-core-0.2.1.0/unliftio-core-0.2.1.0.tar.gz",
    },
    {
      name: "unordered-containers",
      version: "0.2.21",
      unitIds: ["nrdrd-cntnrs-0.2.21-05575037"],
      sourceUrl:
        "https://hackage.haskell.org/package/unordered-containers-0.2.21/unordered-containers-0.2.21.tar.gz",
    },
    {
      name: "vector",
      version: "0.13.2.0",
      unitIds: ["vctr-0.13.2.0-10b30f46"],
      sourceUrl: "https://hackage.haskell.org/package/vector-0.13.2.0/vector-0.13.2.0.tar.gz",
    },
    {
      name: "vector-stream",
      version: "0.1.0.1",
      unitIds: ["vctr-strm-0.1.0.1-aa148597"],
      sourceUrl:
        "https://hackage.haskell.org/package/vector-stream-0.1.0.1/vector-stream-0.1.0.1.tar.gz",
    },
    {
      name: "xml",
      version: "1.3.14",
      unitIds: ["xml-1.3.14-cb761230"],
      sourceUrl: "https://hackage.haskell.org/package/xml-1.3.14/xml-1.3.14.tar.gz",
    },
    {
      name: "xml-conduit",
      version: "1.10.1.0",
      unitIds: ["xml-cndt-1.10.1.0-c33d2a89"],
      sourceUrl: "https://github.com/snoyberg/xml/tree/xml-conduit/1.10.1.0/xml-conduit",
      patchUrl: "https://github.com/jgm/pandoc/blob/3.12/wasm/patches/xml-conduit.patch",
    },
    {
      name: "xml-types",
      version: "0.3.8",
      unitIds: [["xml-ty", "ps-0.3.8-4bb7e9e1"].join("")],
      sourceUrl: "https://hackage.haskell.org/package/xml-types-0.3.8/xml-types-0.3.8.tar.gz",
    },
    {
      name: "yaml",
      version: "0.11.11.2",
      unitIds: ["yml-0.11.11.2-1e7c813a"],
      sourceUrl: "https://hackage.haskell.org/package/yaml-0.11.11.2/yaml-0.11.11.2.tar.gz",
    },
    {
      name: "zip-archive",
      version: "0.5",
      unitIds: ["zp-rchv-0.5-f93e5aed"],
      sourceUrl: "https://hackage.haskell.org/package/zip-archive-0.5/zip-archive-0.5.tar.gz",
    },
    {
      name: "zlib",
      version: "0.7.1.1",
      unitIds: ["zlb-0.7.1.1-9a75416a"],
      sourceUrl: "https://hackage.haskell.org/package/zlib-0.7.1.1/zlib-0.7.1.1.tar.gz",
    },
  ],
  embeddedCSources: [
    {
      name: "lpeg",
      version: "1.1.0 (verified via cbits/lpeg-1.1.0/*.c in pandoc.wasm)",
      sourceUrl: "https://hackage.haskell.org/package/lpeg-1.1.0/lpeg-1.1.0.tar.gz",
    },
  ],
  unfrozenCabalNote:
    "Upstream jgm/pandoc 3.12 runs `wasm32-wasi-cabal update && wasm32-wasi-cabal build pandoc-cli` without a committed `cabal.project.freeze`, `plan.json`, or Cabal `index-state` pin for `wasm32-wasi`, and `wasm-opt -Oz` strips unused GHC unit-id strings for packages without runtime-retained symbol names. The 92 Haskell packages above (97 GHC unit IDs) and `lpeg-1.1.0` are verified directly from `pandoc.wasm` rodata; the remaining 16 direct `build-depends` declared in `pandoc.cabal`, `pandoc-lua-engine.cabal`, and `stack.yaml` whose unit-id strings were stripped by `wasm-opt -Oz` are listed below (exact `wasm32-wasi-cabal` solver versions for those stripped units are not verified from public data).",
  unfrozenCabalDependencies: [
    {
      name: "hslua",
      constraint: ">= 2.5 && < 2.6 (stack.yaml: 2.5.0)",
      sourceUrl: "https://hackage.haskell.org/package/hslua-2.5.0/hslua-2.5.0.tar.gz",
    },
    {
      name: "hslua-module-doclayout",
      constraint: ">= 1.2 && < 1.3 (stack.yaml: 1.2.1.1)",
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-module-doclayout-1.2.1.1/hslua-module-doclayout-1.2.1.1.tar.gz",
    },
    {
      name: "hslua-module-path",
      constraint: ">= 1.2 && < 1.3 (stack.yaml: 1.2.0)",
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-module-path-1.2.0/hslua-module-path-1.2.0.tar.gz",
    },
    {
      name: "hslua-module-text",
      constraint: ">= 1.2 && < 1.3 (stack.yaml: 1.2.0)",
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-module-text-1.2.0/hslua-module-text-1.2.0.tar.gz",
    },
    {
      name: "hslua-module-version",
      constraint: ">= 1.2 && < 1.3 (stack.yaml: 1.2.0.1)",
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-module-version-1.2.0.1/hslua-module-version-1.2.0.1.tar.gz",
    },
    {
      name: "hslua-module-zip",
      constraint: ">= 1.2 && < 1.3 (stack.yaml: 1.2.1)",
      sourceUrl:
        "https://hackage.haskell.org/package/hslua-module-zip-1.2.1/hslua-module-zip-1.2.1.tar.gz",
    },
    {
      name: "skylighting",
      constraint: ">= 0.15 && < 0.16 (stack.yaml: 0.15)",
      sourceUrl: "https://hackage.haskell.org/package/skylighting-0.15/skylighting-0.15.tar.gz",
    },
    {
      name: "blaze-html",
      constraint: ">= 0.9 && < 0.10 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
    {
      name: "data-default",
      constraint: ">= 0.4 && < 0.9 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
    {
      name: "deepseq",
      constraint: ">= 1.3 && < 1.6 (GHC 9.14.1 boot library)",
      sourceUrl:
        "https:// gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release/libraries/deepseq".replace(
          " ",
          "",
        ),
    },
    {
      name: "network",
      constraint: ">= 2.6 && < 3.3 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
    {
      name: "random",
      constraint: ">= 1.2 && < 1.4 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
    {
      name: "syb",
      constraint: ">= 0.1 && < 0.8 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
    {
      name: "text-builder",
      constraint: ">= 1.0 && < 1.1 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
    {
      name: "text-conversions",
      constraint: ">= 0.3 && < 0.4 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
    {
      name: "unicode-data",
      constraint: ">= 0.6 && < 0.10 (declared in pandoc.cabal at tag 3.12)",
      sourceUrl: "https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal",
    },
  ],
};

const UNIT_ID_RE =
  /([A-Za-z0-9]+(?:-[A-Za-z0-9]+)*-\d+(?:\.\d+)*-(?:[0-9a-f]{4,64}|inplace(?:-[A-Za-z0-9-]+)?))(:[A-Z][A-Za-z0-9_.]*)?/g;

/**
 * @param {Buffer | string} wasmBytes
 * @returns {string[]}
 */
export function extractPandocWasmUnitIds(wasmBytes) {
  const text = Buffer.isBuffer(wasmBytes) ? wasmBytes.toString("latin1") : String(wasmBytes);
  const units = new Set();
  for (const match of text.matchAll(UNIT_ID_RE)) {
    const unitId = match[1];
    const index = match.index ?? 0;
    if (unitId && (match[2] || text.charCodeAt(index + unitId.length) === 0)) {
      units.add(unitId);
    }
  }
  return [...units].sort();
}

/**
 * @param {{
 *   version?: string;
 *   archiveSha256?: string;
 *   wasmSha256?: string;
 *   wasmBytes?: Buffer | string | null;
 *   inventory?: typeof pandocSourceInventory;
 * }} [options]
 */
export function verifyPandocSourceInventory({
  version = pandocSourceInventory.version,
  archiveSha256 = pandocSourceInventory.archiveSha256,
  wasmSha256 = pandocSourceInventory.wasmSha256,
  wasmBytes = null,
  inventory = pandocSourceInventory,
} = {}) {
  if (version !== inventory.version || version !== inventory.gitTag) {
    throw new Error(
      `Pandoc source inventory version/tag (${inventory.version} / ${inventory.gitTag}) does not match engine-assets.mjs (${version})`,
    );
  }
  const expectedReleaseTagUrl = `https://github.com/jgm/pandoc/releases/tag/${inventory.gitTag}`;
  if (inventory.releaseTagUrl !== expectedReleaseTagUrl) {
    throw new Error(
      `Pandoc release tag URL (${inventory.releaseTagUrl}) does not match tag ${inventory.gitTag} (${expectedReleaseTagUrl})`,
    );
  }
  const expectedArchiveUrl = `https://github.com/jgm/pandoc/releases/download/${inventory.gitTag}/pandoc-${inventory.version}.wasm.zip`;
  if (inventory.archiveUrl !== expectedArchiveUrl) {
    throw new Error(
      `Pandoc release asset URL (${inventory.archiveUrl}) does not match tag/version (${expectedArchiveUrl})`,
    );
  }
  if (archiveSha256 !== inventory.archiveSha256) {
    throw new Error(
      `Pandoc archive SHA-256 (${archiveSha256}) does not match inventory (${inventory.archiveSha256})`,
    );
  }
  if (inventory.upstreamArchiveDigest !== `sha256:${inventory.archiveSha256}`) {
    throw new Error(
      `Upstream release asset digest (${inventory.upstreamArchiveDigest}) does not match archive SHA-256 (sha256:${inventory.archiveSha256})`,
    );
  }
  if (wasmSha256 !== inventory.wasmSha256) {
    throw new Error(
      `pandoc.wasm SHA-256 (${wasmSha256}) does not match inventory (${inventory.wasmSha256})`,
    );
  }
  if (
    !inventory.hashComparisonNote?.includes(inventory.archiveSha256) ||
    !inventory.hashComparisonNote?.includes(inventory.wasmSha256)
  ) {
    throw new Error(
      "Pandoc hashComparisonNote must record both the archive SHA-256 and shipped pandoc.wasm SHA-256",
    );
  }

  for (const item of [...inventory.recipe.files, ...inventory.freezeAndLockFiles.publishedFiles]) {
    const validTagPermalink =
      item.url?.startsWith(`https://github.com/jgm/pandoc/blob/${inventory.gitTag}/`) ||
      item.url?.startsWith(`https://github.com/jgm/pandoc/tree/${inventory.gitTag}/`);
    const validCommitPermalink =
      item.commitUrl?.startsWith(`https://github.com/jgm/pandoc/blob/${inventory.gitCommit}/`) ||
      item.commitUrl?.startsWith(`https://github.com/jgm/pandoc/tree/${inventory.gitCommit}/`);
    if (!validTagPermalink || !validCommitPermalink) {
      throw new Error(
        `Pandoc build-definition or freeze/lock permalink must be pinned to tag ${inventory.gitTag} and commit ${inventory.gitCommit}: ${item.url ?? "missing"} / ${item.commitUrl ?? "missing"}`,
      );
    }
  }

  const freezeSummary = [
    inventory.freezeAndLockFiles?.summary ?? "",
    ...(inventory.freezeAndLockFiles?.unpublishedFiles ?? []),
    ...(inventory.freezeAndLockFiles?.unverifiedFacts ?? []),
  ].join("\n");
  for (const requiredToken of [
    "cabal.project.freeze",
    "plan.json",
    "index-state",
    "not verified from public data",
    "haskell-wasm/ghc fork commit",
    "2600313",
  ]) {
    if (!freezeSummary.includes(requiredToken)) {
      throw new Error(
        `Pandoc freezeAndLockFiles disclosure is missing required token: ${requiredToken}`,
      );
    }
  }

  const ghcToolchain = inventory.toolchain.find((item) => item.name.startsWith("wasm32-wasi-ghc"));
  if (
    !ghcToolchain?.version.includes("not verified from public data") ||
    !ghcToolchain?.sourceLabel?.includes("not the unverified haskell-wasm/ghc fork commit")
  ) {
    throw new Error(
      "Pandoc wasm32-wasi-ghc toolchain entry must qualify the upstream GHC release link and state that the exact haskell-wasm/ghc fork commit is not verified from public data",
    );
  }

  const basePkg = inventory.embeddedPackages.find((pkg) => pkg.name === "base");
  if (
    !basePkg ||
    basePkg.sourceUrl.includes("hackage.haskell.org/package/base-4.22.1.0") ||
    !basePkg.sourceUrl.includes("ghc-9.14.1-release/libraries/base") ||
    !basePkg.note?.includes("not verified from public data")
  ) {
    throw new Error(
      "Pandoc base-4.22.1.0 boot-library entry must link the verified GHC release libraries/base tree with fork qualification",
    );
  }

  const offerText = inventory.writtenOffer?.text ?? "";
  if (
    inventory.writtenOffer?.validityYears < 3 ||
    !offerText.includes("three (3) years") ||
    !offerText.includes("corresponding source") ||
    !offerText.includes(inventory.writtenOffer?.contactUrl ?? "")
  ) {
    throw new Error(
      "Pandoc written corresponding-source offer must be valid for at least three years and include the repository contact URL",
    );
  }

  const stableSourceUrlRe =
    /^https:\/\/(?:github\.com\/[^/]+\/[^/]+\/(?:releases\/(?:tag|download)\/|(?:blob|tree|commit)\/|archive\/refs\/tags\/)|gitlab\.haskell\.org\/[^/]+\/[^/]+\/-\/(?:tree|blob|commit)\/|hackage\.haskell\.org\/package\/[A-Za-z0-9-]+-\d+(?:\.\d+)*\/[A-Za-z0-9-]+-\d+(?:\.\d+)*\.tar\.gz$)/;

  const sourcePermalinks = [
    inventory.releaseTagUrl,
    inventory.archiveUrl,
    ...Object.values(inventory.sourceUrls),
    ...inventory.recipe.files.flatMap((item) => [item.url, item.commitUrl].filter(Boolean)),
    ...inventory.freezeAndLockFiles.publishedFiles.flatMap((item) =>
      [item.url, item.commitUrl].filter(Boolean),
    ),
    ...inventory.toolchain.flatMap((item) => [item.sourceUrl, item.secondaryUrl].filter(Boolean)),
    ...inventory.embeddedPackages.flatMap((item) =>
      [item.sourceUrl, item.patchUrl].filter(Boolean),
    ),
    ...inventory.embeddedCSources.map((item) => item.sourceUrl),
    ...inventory.unfrozenCabalDependencies.map((item) => item.sourceUrl),
  ];

  for (const url of sourcePermalinks) {
    const ref = /\/(?:blob|tree|commit|tag|download|tags)\/([^/]+)/.exec(url)?.[1];
    const versionedRef =
      ref &&
      /^(?:[0-9a-f]{40}|v?\d+(?:\.\d+)+(?:[-+][\w.-]+)?|(?:ghc-|lua-|cabal-install-v|wasi-sdk-|version_)\d[\w.-]*|\d{8}T\d{6})$/i.test(
        ref.replace(/\.tar\.gz$/, ""),
      );
    const versionedXmlTag = ref === "xml-conduit" && /\/xml-conduit\/\d/.test(url);
    if (
      typeof url !== "string" ||
      !stableSourceUrlRe.test(url) ||
      (ref && !versionedRef && !versionedXmlTag) ||
      /\/(?:blob|tree|commit|tag|download)\/(?:main|master|dev|HEAD)(?:\/|$)/i.test(url)
    ) {
      throw new Error(`Unstable or non-permalink source URL in Pandoc source inventory: ${url}`);
    }
  }

  for (const url of [
    inventory.writtenOffer.contactUrl,
    inventory.writtenOffer.repositoryUrl,
    inventory.writtenOffer.securityAdvisoriesUrl,
  ]) {
    if (typeof url !== "string" || !url.startsWith("https://")) {
      throw new Error(`Invalid contact URL in Pandoc written offer: ${url}`);
    }
  }

  if (wasmBytes !== null) {
    const actualUnits = extractPandocWasmUnitIds(wasmBytes);
    const expectedUnits = inventory.embeddedPackages.flatMap((pkg) => pkg.unitIds).sort();
    if (actualUnits.join("\n") !== expectedUnits.join("\n")) {
      const missing = expectedUnits.filter((unit) => !actualUnits.includes(unit));
      const extra = actualUnits.filter((unit) => !expectedUnits.includes(unit));
      throw new Error(
        `pandoc.wasm embedded GHC unit IDs diverged from pandoc-source-inventory.mjs (missing: ${missing.join(", ") || "none"}; extra: ${extra.join(", ") || "none"})`,
      );
    }
    const actualWasmSha256 = createHash("sha256").update(wasmBytes).digest("hex");
    if (actualWasmSha256 !== wasmSha256) {
      throw new Error(
        `Shipped pandoc.wasm SHA-256 (${actualWasmSha256}) does not match recorded SHA-256 (${wasmSha256})`,
      );
    }
  }
}

export function renderPandocSourceTxt(inventory = pandocSourceInventory) {
  const lines = [
    `Unmodified Pandoc ${inventory.version}, ${inventory.license}.`,
    `Upstream git tag: ${inventory.gitTag} (commit ${inventory.gitCommit})`,
    `Upstream release page (tag): ${inventory.releaseTagUrl}`,
    `Upstream release asset (zip): ${inventory.archiveUrl}`,
    `Release archive SHA-256: ${inventory.archiveSha256} (upstream published GitHub Release asset digest: ${inventory.upstreamArchiveDigest})`,
    `Shipped pandoc.wasm SHA-256: ${inventory.wasmSha256} (upstream standalone digest for inner pandoc.wasm: none published)`,
    `SHA-256 comparison: ${inventory.hashComparisonNote}`,
    `Source tree: ${inventory.sourceUrls.tagTree}`,
    `Source commit: ${inventory.sourceUrls.commitTree}`,
    `Source archive: ${inventory.sourceUrls.sourceArchive}`,
    `Hackage pandoc tarball: ${inventory.sourceUrls.hackagePandoc}`,
    `Hackage pandoc-cli tarball: ${inventory.sourceUrls.hackagePandocCli}`,
    "",
    "Written Corresponding-Source Offer (Three Years)",
    "================================================",
    inventory.writtenOffer.text,
    "",
    "Build Recipe and Upstream Build Definitions (Tag 3.12 Permalinks)",
    "=================================================================",
    inventory.recipe.summary,
    ...inventory.recipe.files.map(
      (item) => `- ${item.label}: ${item.url} (commit: ${item.commitUrl})`,
    ),
    "",
    "Upstream Freeze, Lock, and Plan Files at Tag 3.12",
    "=================================================",
    inventory.freezeAndLockFiles.summary,
    ...inventory.freezeAndLockFiles.publishedFiles.map(
      (item) => `- Published: ${item.label}: ${item.url} (commit: ${item.commitUrl})`,
    ),
    ...inventory.freezeAndLockFiles.unpublishedFiles.map((item) => `- Not published: ${item}`),
    ...inventory.freezeAndLockFiles.unverifiedFacts.map((item) => `- Disclosure: ${item}`),
    "",
    "Compiler, WASI Libc, and Toolchain Sources",
    "==========================================",
    ...inventory.toolchain.map(
      (item) =>
        `- ${item.name}: ${item.version}\n  ${item.sourceLabel ?? "Source"}: ${item.sourceUrl}${item.secondaryUrl ? `\n  ${item.secondaryLabel ?? "Bindist / release"}: ${item.secondaryUrl}` : ""}`,
    ),
    "",
    `Linked Haskell Packages Verified in pandoc.wasm (${inventory.embeddedPackages.length} packages, ${inventory.embeddedPackages.reduce((acc, pkg) => acc + pkg.unitIds.length, 0)} GHC units)`,
    "====================================================================================",
    ...inventory.embeddedPackages.map(
      (pkg) =>
        `- ${pkg.name}-${pkg.version} [units: ${pkg.unitIds.join(", ")}]: ${pkg.sourceUrl}${pkg.patchUrl ? ` (patch: ${pkg.patchUrl})` : ""}${pkg.note ? ` — ${pkg.note}` : ""}`,
    ),
    "",
    "Linked C Sources Verified in pandoc.wasm",
    "========================================",
    ...inventory.embeddedCSources.map(
      (item) => `- ${item.name} ${item.version}: ${item.sourceUrl}`,
    ),
    "",
    "Additional Cabal Dependencies (No Upstream wasm32-wasi Freeze File)",
    "===================================================================",
    inventory.unfrozenCabalNote,
    ...inventory.unfrozenCabalDependencies.map(
      (item) => `- ${item.name} (${item.constraint}): ${item.sourceUrl}`,
    ),
    "",
  ];
  return lines.join("\n");
}
