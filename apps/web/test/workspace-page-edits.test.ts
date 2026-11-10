import {
  createPageEditStore,
  isDirty,
  planPageEdits,
  remainingPages,
  toPageList,
} from "@/components/workspace/page-edits";

const order = (store: ReturnType<typeof createPageEditStore>) =>
  store.getState().pages.map((entry) => entry.page);

describe("page edit model", () => {
  it("moves a page, and undo and redo walk the history", () => {
    const store = createPageEditStore(4);
    store.getState().move(0, 2);
    expect(order(store)).toEqual([2, 3, 1, 4]);
    expect(store.getState().past).toHaveLength(1);
    store.getState().move(3, 0);
    expect(order(store)).toEqual([4, 2, 3, 1]);

    store.getState().undo();
    expect(order(store)).toEqual([2, 3, 1, 4]);
    store.getState().undo();
    expect(order(store)).toEqual([1, 2, 3, 4]);
    expect(isDirty(store.getState())).toBe(false);
    store.getState().redo();
    expect(order(store)).toEqual([2, 3, 1, 4]);
    // A new edit discards the redo branch.
    store.getState().rotate([1], 90);
    expect(store.getState().future).toHaveLength(0);
  });

  it("rotates in either direction and wraps around", () => {
    const store = createPageEditStore(2);
    store.getState().rotate([1], -90);
    expect(store.getState().pages[0]?.rotation).toBe(270);
    store.getState().rotate([1], 90);
    expect(store.getState().pages[0]?.rotation).toBe(0);
    store.getState().rotate([2], 180);
    expect(store.getState().pages[1]?.rotation).toBe(180);
  });

  it("deletes and restores, ignores no-ops, and resets everything", () => {
    const store = createPageEditStore(3);
    store.getState().remove([2]);
    store.getState().remove([2]);
    expect(store.getState().past).toHaveLength(1);
    expect(remainingPages(store.getState().pages)).toBe(2);
    store.getState().restore([2]);
    expect(isDirty(store.getState())).toBe(false);
    store.getState().remove([1, 3]);
    store.getState().reset();
    expect(store.getState().past).toHaveLength(0);
    expect(order(store)).toEqual([1, 2, 3]);
    expect(isDirty(store.getState())).toBe(false);
  });

  it("nudges a page and clamps at the ends", () => {
    const store = createPageEditStore(3);
    expect(store.getState().nudge(1, 1)).toBe(1);
    expect(order(store)).toEqual([2, 1, 3]);
    expect(store.getState().nudge(2, -5)).toBe(0);
    expect(order(store)).toEqual([2, 1, 3]);
    expect(store.getState().past).toHaveLength(1);
  });

  it("selects a range in the current order from the anchor", () => {
    const store = createPageEditStore(5);
    store.getState().move(4, 0);
    store.getState().select(2, "toggle");
    store.getState().select(1, "range");
    expect([...store.getState().selected].sort()).toEqual([1, 2]);
    store.getState().clearSelection();
    store.getState().selectRange(5, 3);
    expect([...store.getState().selected].sort()).toEqual([1, 2, 3, 5]);
  });
});

describe("page edit planning", () => {
  it("formats page lists, collapsing only ascending runs", () => {
    expect(toPageList([5, 1, 2, 3, 7], { sort: true })).toBe("1-3,5,7");
    expect(toPageList([3, 2, 1], { sort: false })).toBe("3,2,1");
    expect(toPageList([1, 2, 3, 6, 7, 4], { sort: false })).toBe("1-3,6-7,4");
  });

  it("plans nothing for an unchanged document", () => {
    expect(planPageEdits(createPageEditStore(3).getState().pages)).toEqual([]);
  });

  it("plans rotate, then delete, then reorder, addressing the right page numbers", () => {
    const store = createPageEditStore(6);
    store.getState().rotate([2, 3], 90);
    store.getState().rotate([5], -90);
    store.getState().rotate([6], 180);
    store.getState().remove([4]);
    store.getState().move(0, 4);
    // Order is now 2,3,4,5,1,6 with page 4 deleted.
    expect(planPageEdits(store.getState().pages)).toEqual([
      { toolId: "pdf.rotate", params: { angle: 90, pages: "2-3" } },
      { toolId: "pdf.rotate", params: { angle: 180, pages: "6" } },
      { toolId: "pdf.rotate", params: { angle: 270, pages: "5" } },
      { toolId: "pdf.delete_pages", params: { pages: "4" } },
      // After deleting page 4 the survivors are 1,2,3,5,6 → 1..5; the order 2,3,5,1,6 is 2,3,4,1,5.
      { toolId: "pdf.reorder", params: { order: "2-4,1,5" } },
    ]);
  });

  it("does not rotate a page that is deleted", () => {
    const store = createPageEditStore(3);
    store.getState().rotate([2], 90);
    store.getState().remove([2]);
    expect(planPageEdits(store.getState().pages)).toEqual([
      { toolId: "pdf.delete_pages", params: { pages: "2" } },
    ]);
  });

  it("refuses to delete every page", () => {
    const store = createPageEditStore(2);
    store.getState().remove([1, 2]);
    expect(() => planPageEdits(store.getState().pages)).toThrow(/at least one page/);
  });
});
