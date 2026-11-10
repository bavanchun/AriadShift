"use client";
// Builds the workspace's translated strings from the message catalogue (`web.workspace`).
// The components read them through `useWorkspaceCopy`, so they stay free of next-intl.
import { useLocale, useTranslations } from "next-intl";
import { useMemo } from "react";
import { toolTitle } from "../../lib/tools";
import type { ViewerLabels } from "../../viewer/types";
import { DEFAULT_WORKSPACE_COPY, mergeWorkspaceCopy, type WorkspaceCopy } from "./copy";

type Values = Record<string, string | number>;

/** Translated workspace copy and viewer labels for the active locale. */
export function useWorkspaceIntl(): { copy: WorkspaceCopy; viewerLabels: ViewerLabels } {
  const locale = useLocale();
  const t = useTranslations("web.workspace");
  const tt = useTranslations("web.tools");
  return useMemo(() => {
    const number = new Intl.NumberFormat(locale);
    const s = (key: string, values?: Values) => t(key, values);
    const fileTray: WorkspaceCopy["fileTray"] = {
      ...DEFAULT_WORKSPACE_COPY.fileTray,
      privacy: s("file_tray.privacy"),
      title: s("file_tray.title"),
      empty: s("file_tray.empty"),
      choose: s("file_tray.choose"),
      addMore: s("file_tray.add_more"),
      dropTitle: s("file_tray.drop_title"),
      dropDescription: s("file_tray.drop_description"),
      pages: (count) => s("file_tray.pages", { count }),
      remove: (name) => s("file_tray.remove", { name }),
      moveUp: (name) => s("file_tray.move_up", { name }),
      moveDown: (name) => s("file_tray.move_down", { name }),
      use: (name) => s("file_tray.use", { name }),
      active: s("file_tray.active"),
      storage: (kind) => s(kind === "opfs" ? "file_tray.storage_opfs" : "file_tray.storage_memory"),
      reading: s("file_tray.reading"),
      list: s("file_tray.list"),
      problem: (name) => s("file_tray.problem", { name }),
    };
    const insights: WorkspaceCopy["insights"] = {
      ...DEFAULT_WORKSPACE_COPY.insights,
      title: s("insights.title"),
      loading: s("insights.loading"),
      failed: s("insights.failed"),
      pages: (count) => s("insights.pages", { count }),
      scanned: (count) => s("insights.scanned", { count }),
      blank: (count) => s("insights.blank", { count }),
      dpi: (min, max) =>
        min === null || max === null
          ? s("insights.dpi_unknown")
          : min === max
            ? s("insights.dpi_single", { dpi: number.format(min) })
            : s("insights.dpi_range", { min: number.format(min), max: number.format(max) }),
      lowDpi: (pages) =>
        s("insights.low_dpi", {
          count: pages.length,
          list: `${pages.slice(0, 6).join(", ")}${pages.length > 6 ? "…" : ""}`,
        }),
      formFields: (count) => s("insights.form_fields", { count }),
      signatures: (count) => s("insights.signatures", { count }),
      metadata: s("insights.metadata"),
      editHistory: s("insights.edit_history"),
      encrypted: s("insights.encrypted"),
      linearized: s("insights.linearized"),
      clean: s("insights.clean"),
      suggested: s("insights.suggested"),
      noSuggestions: s("insights.no_suggestions"),
      open: (tool) => s("insights.open", { tool }),
    };
    const pageGrid: WorkspaceCopy["pageGrid"] = {
      ...DEFAULT_WORKSPACE_COPY.pageGrid,
      title: s("page_grid.title"),
      toolbar: s("page_grid.toolbar"),
      undo: s("page_grid.undo"),
      redo: s("page_grid.redo"),
      reset: s("page_grid.reset"),
      pending: (count) => s("page_grid.pending", { count }),
      selectAll: s("page_grid.select_all"),
      clearSelection: s("page_grid.clear_selection"),
      selectRange: s("page_grid.select_range"),
      rangePlaceholder: s("page_grid.range_placeholder"),
      rotateLeft: s("page_grid.rotate_left"),
      rotateRight: s("page_grid.rotate_right"),
      deleteSelected: s("page_grid.delete_selected"),
      restoreSelected: s("page_grid.restore_selected"),
      apply: s("page_grid.apply"),
      hint: s("page_grid.hint"),
      dragHint: s("page_grid.drag_hint"),
      moved: (page, position) => s("page_grid.moved", { page, position }),
      cannotDeleteAll: s("page_grid.cannot_delete_all"),
      cardPage: (page) => s("page_grid.card_page", { page }),
      cardSelect: (page) => s("page_grid.card_select", { page }),
      cardRotate: (page) => s("page_grid.card_rotate", { page }),
      cardDelete: (page) => s("page_grid.card_delete", { page }),
      cardMove: (page) => s("page_grid.card_move", { page }),
      cardRestore: (page) => s("page_grid.card_restore", { page }),
      blank: s("page_grid.blank"),
      scanned: s("page_grid.scanned"),
    };
    const inspector: WorkspaceCopy["inspector"] = {
      ...DEFAULT_WORKSPACE_COPY.inspector,
      title: s("inspector.title"),
      empty: s("inspector.empty"),
      runtime: {
        browser: s("inspector.runtime.browser"),
        desktop: s("inspector.runtime.desktop"),
        cloud: s("inspector.runtime.cloud"),
      },
      tier: { free: s("inspector.tier.free"), pro: s("inspector.tier.pro") },
      runHere: s("inspector.run_here"),
      running: s("inspector.running"),
      cancel: s("inspector.cancel"),
      runCloud: s("inspector.run_cloud"),
      cloudNote: s("inspector.cloud_note"),
      switchDesktop: s("inspector.switch_desktop"),
      switchBrowser: s("inspector.switch_browser"),
      needsFile: s("inspector.needs_file"),
      needsPdf: s("inspector.needs_pdf"),
      invalid: s("inspector.invalid"),
      failed: (message) => s("inspector.failed", { message }),
      progress: s("inspector.progress"),
    };
    const batch: WorkspaceCopy["batch"] = {
      title: s("batch.title"),
      empty: s("batch.empty"),
      overall: (completed, total) => s("batch.overall", { completed, total }),
      sizes: (input, output) => s("batch.sizes", { input, output }),
      status: {
        queued: s("batch.status.queued"),
        running: s("batch.status.running"),
        done: s("batch.status.done"),
        failed: s("batch.status.failed"),
        cancelled: s("batch.status.cancelled"),
      },
      runBatch: s("batch.run_batch"),
      cancelBatch: s("batch.cancel_batch"),
      retryFailed: s("batch.retry_failed"),
      downloadZip: s("batch.download_zip"),
      clearCompleted: s("batch.clear_completed"),
      list: s("batch.list"),
      download: (name) => s("batch.download", { name }),
      cancelItem: (name) => s("batch.cancel_item", { name }),
      removeItem: (name) => s("batch.remove_item", { name }),
      guardTitle: s("batch.guard_title"),
      guardBody: (limit, names) => s("batch.guard_body", { limit, names }),
      toggle: s("batch.toggle"),
      invalid: s("batch.invalid"),
      failed: (message) => s("batch.failed", { message }),
    };
    const handoff: WorkspaceCopy["handoff"] = {
      title: s("handoff.title"),
      why: s("handoff.why"),
      command: s("handoff.command"),
      copy: s("handoff.copy"),
      copied: s("handoff.copied"),
      copyFailed: s("handoff.copy_failed"),
      install: s("handoff.install"),
      openApp: s("handoff.open_app"),
      revealSecrets: s("handoff.reveal_secrets"),
      maskSecrets: s("handoff.mask_secrets"),
      secretNote: s("handoff.secret_note"),
    };
    const result: WorkspaceCopy["result"] = {
      ...DEFAULT_WORKSPACE_COPY.result,
      title: s("result.title"),
      before: s("result.before"),
      after: s("result.after"),
      size: s("result.size"),
      pages: s("result.pages"),
      duration: s("result.duration"),
      change: (percent) =>
        percent === 0
          ? s("result.change_none")
          : s(percent > 0 ? "result.change_up" : "result.change_down", {
              percent: number.format(Math.abs(percent)),
            }),
      bytes: (value) => s("result.bytes", { value: number.format(value) }),
      seconds: (ms) =>
        ms < 1000
          ? s("result.millis", { value: number.format(Math.round(ms)) })
          : s("result.seconds", {
              value: new Intl.NumberFormat(locale, {
                minimumFractionDigits: 1,
                maximumFractionDigits: 1,
              }).format(ms / 1000),
            }),
      warnings: s("result.warnings"),
      details: s("result.details"),
      download: (name) => s("result.download", { name }),
      openInEditor: s("result.open_in_editor"),
      continue: s("result.continue"),
      saveRecipe: s("result.save_recipe"),
      undo: s("result.undo"),
      outputs: s("result.outputs"),
      downloadFailed: s("result.download_failed"),
    };
    const thread: WorkspaceCopy["thread"] = {
      label: s("thread.label"),
      status: {
        pending: s("thread.status.pending"),
        running: s("thread.status.running"),
        completed: s("thread.status.completed"),
        skipped: s("thread.status.skipped"),
        failed: s("thread.status.failed"),
      },
    };
    const main: WorkspaceCopy["workspace"] = {
      title: s("main.title"),
      lede: s("main.lede"),
      viewer: s("main.viewer"),
      pagesTab: s("main.pages_tab"),
      toolsTab: s("main.tools_tab"),
      noPdf: s("main.no_pdf"),
      tools: s("main.tools"),
      toolPicker: s("main.tool_picker"),
      dismissError: s("main.dismiss_error"),
      working: s("main.working"),
    };
    const tools: WorkspaceCopy["tools"] = {
      title: (id) => {
        const key = `title.${id}`;
        return tt.has(key) ? tt(key) : toolTitle({ id });
      },
    };
    const viewerLabels: ViewerLabels = {
      zoomIn: s("viewer.zoom_in"),
      zoomOut: s("viewer.zoom_out"),
      fitWidth: s("viewer.fit_width"),
      fitPage: s("viewer.fit_page"),
      zoomLevel: (percent) => s("viewer.zoom_level", { percent: number.format(percent) }),
      page: s("viewer.page"),
      pageOf: (count) => s("viewer.page_of", { count: number.format(count) }),
      search: s("viewer.search"),
      searchPlaceholder: s("viewer.search_placeholder"),
      searchRun: s("viewer.search_run"),
      searchCancel: s("viewer.search_cancel"),
      searchClear: s("viewer.search_clear"),
      searchNext: s("viewer.search_next"),
      searchPrevious: s("viewer.search_previous"),
      searching: (percent) => s("viewer.searching", { percent: number.format(percent) }),
      matches: (current, total) =>
        s("viewer.matches", { current: number.format(current), total: number.format(total) }),
      noMatches: s("viewer.no_matches"),
      darkPages: s("viewer.dark_pages"),
      loading: s("viewer.loading"),
      unavailable: s("viewer.unavailable"),
      documentError: s("viewer.document_error"),
      toolbar: s("viewer.toolbar"),
    };
    return {
      copy: mergeWorkspaceCopy({
        fileTray,
        insights,
        pageGrid,
        inspector,
        batch,
        handoff,
        result,
        thread,
        workspace: main,
        tools,
      }),
      viewerLabels,
    };
  }, [locale, t, tt]);
}
