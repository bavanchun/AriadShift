import { useEffect } from "react";
import {
  INITIAL_VIEWER_SNAPSHOT,
  type ViewerBackend,
  type ViewerBackendProps,
  type ViewerCommands,
  type ViewerSnapshot,
} from "@/viewer/types";

export const readySnapshot = (overrides: Partial<ViewerSnapshot> = {}): ViewerSnapshot => ({
  ...INITIAL_VIEWER_SNAPSHOT,
  status: "ready",
  page: 1,
  pageCount: 10,
  zoomPercent: 100,
  zoomMode: "fit-width",
  ...overrides,
});

export function fakeCommands(): ViewerCommands {
  return {
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    setZoomMode: vi.fn(),
    goToPage: vi.fn(),
    search: vi.fn(),
    nextMatch: vi.fn(),
    previousMatch: vi.fn(),
    cancelSearch: vi.fn(),
    clearSearch: vi.fn(),
  };
}

/** A backend whose viewer reports what the test tells it to, and records its lifecycle. */
export function fakeBackend(initial: ViewerSnapshot = readySnapshot()) {
  const commands = fakeCommands();
  const log = { mounts: 0, unmounts: 0, props: [] as ViewerBackendProps[] };
  const controls: { emit: (snapshot: ViewerSnapshot) => void } = { emit: () => undefined };
  function Viewer(props: ViewerBackendProps) {
    const { onSnapshot, onCommands } = props;
    log.props.push(props);
    controls.emit = onSnapshot;
    useEffect(() => {
      log.mounts += 1;
      onSnapshot(initial);
      onCommands(commands);
      return () => {
        log.unmounts += 1;
        onCommands(null);
      };
    }, [onSnapshot, onCommands]);
    return (
      <div data-testid="engine" data-invert={String(props.invert)} data-source={props.source.id} />
    );
  }
  const backend: ViewerBackend = {
    Viewer,
    PagePreviewProvider: ({ children }) => (
      <>
        {children((page) => (
          <span data-testid={`preview-${page}`} />
        ))}
      </>
    ),
  };
  return { backend, commands, log, controls, load: vi.fn(async () => backend) };
}
