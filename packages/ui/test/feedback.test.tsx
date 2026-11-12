import { act, fireEvent, render, screen } from "@testing-library/react";
import { Progress } from "../src/components/progress";
import { Skeleton } from "../src/components/skeleton";
import {
  Toast,
  ToastAction,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
  useToast,
} from "../src/components/toast";

function Trigger({
  options,
  label = "Notify",
}: {
  options: Parameters<ReturnType<typeof useToast>["toast"]>[0];
  label?: string;
}) {
  const { toast } = useToast();
  return (
    <button type="button" onClick={() => toast(options)}>
      {label}
    </button>
  );
}

function setup(
  options: Parameters<ReturnType<typeof useToast>["toast"]>[0],
  providerDuration?: number,
) {
  return render(
    <ToastProvider duration={providerDuration}>
      <Trigger options={options} />
      <Trigger options={{ title: "Second" }} label="Other" />
      <ToastViewport />
    </ToastProvider>,
  );
}

function Many() {
  const { toast, dismiss } = useToast();
  return (
    <>
      <button
        type="button"
        onClick={() => {
          for (let i = 1; i <= 7; i += 1)
            toast({ title: `n${i}`, duration: Number.POSITIVE_INFINITY });
        }}
      >
        Many
      </button>
      <button type="button" onClick={() => dismiss()}>
        Clear
      </button>
    </>
  );
}

function ById() {
  const { toast, dismiss } = useToast();
  return (
    <button
      type="button"
      onClick={() => {
        const first = toast({ title: "one", duration: Number.POSITIVE_INFINITY });
        toast({ title: "two", duration: Number.POSITIVE_INFINITY });
        dismiss(first);
      }}
    >
      Go
    </button>
  );
}

describe("Toast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders into a labelled live region with title and description", () => {
    setup({ title: "Saved", description: "1.2 MB" });
    const region = screen.getByRole("region", { name: "Notifications" });
    expect(region.getAttribute("aria-live")).toBe("polite");
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    expect(screen.getByRole("status").textContent).toContain("Saved");
    expect(screen.getByRole("status").textContent).toContain("1.2 MB");
  });

  it.each([
    ["default", "status"],
    ["success", "status"],
    ["warning", "alert"],
    ["destructive", "alert"],
  ] as const)("uses the %s variant with the %s role", (variant, role) => {
    setup({ title: "T", variant });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    const toast = screen.getByRole(role);
    expect(toast.getAttribute("data-variant")).toBe(variant);
  });

  it("dismisses itself after its duration", () => {
    setup({ title: "Bye", duration: 1000 });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    expect(screen.getByText("Bye")).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(999);
    });
    expect(screen.queryByText("Bye")).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(2);
    });
    expect(screen.queryByText("Bye")).toBeNull();
  });

  it("uses the provider duration by default", () => {
    setup({ title: "Quick" }, 500);
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    act(() => {
      vi.advanceTimersByTime(501);
    });
    expect(screen.queryByText("Quick")).toBeNull();
  });

  it("stays when the duration is Infinity", () => {
    setup({ title: "Sticky", duration: Number.POSITIVE_INFINITY });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.getByText("Sticky")).toBeTruthy();
  });

  it("pauses while hovered and resumes with the remaining time", () => {
    setup({ title: "Hold", duration: 1000 });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    const toast = screen.getByRole("status");
    act(() => {
      vi.advanceTimersByTime(600);
    });
    fireEvent.pointerEnter(toast);
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(screen.getByText("Hold")).toBeTruthy();
    fireEvent.pointerLeave(toast);
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByText("Hold")).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(150);
    });
    expect(screen.queryByText("Hold")).toBeNull();
  });

  it("pauses while focus is inside", () => {
    setup({ title: "Focus", duration: 1000 });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    fireEvent.focus(screen.getByRole("button", { name: "Dismiss notification" }));
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(screen.getByText("Focus")).toBeTruthy();
  });

  it("closes from the close button", () => {
    setup({ title: "Close me" });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss notification" }));
    expect(screen.queryByText("Close me")).toBeNull();
  });

  it("runs an action handler and then dismisses the toast", () => {
    const onUndo = vi.fn();
    setup({ title: "Removed", action: <ToastAction onClick={onUndo}>Undo</ToastAction> });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(onUndo).toHaveBeenCalledOnce();
    expect(screen.queryByText("Removed")).toBeNull();
  });

  it("shows the Golden Thread countdown only when asked", () => {
    const { container } = setup({ title: "Thread", thread: true, duration: 2000 });
    fireEvent.click(screen.getByRole("button", { name: "Notify" }));
    const thread = container.querySelector('[data-slot="toast-thread"]') as HTMLElement;
    expect(thread.style.animationDuration).toBe("2000ms");
    expect(thread.getAttribute("aria-hidden")).toBe("true");
  });

  it("keeps the five newest toasts and dismisses all on demand", () => {
    render(
      <ToastProvider>
        <Many />
        <ToastViewport />
      </ToastProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Many" }));
    expect(screen.queryByText("n2")).toBeNull();
    expect(screen.getByText("n3")).toBeTruthy();
    expect(screen.getByText("n7")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.queryByText("n7")).toBeNull();
  });

  it("dismisses one toast by id", () => {
    render(
      <ToastProvider>
        <ById />
        <ToastViewport label="Alerts" />
      </ToastProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(screen.queryByText("one")).toBeNull();
    expect(screen.getByText("two")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Alerts" })).toBeTruthy();
  });

  it("works standalone with composed parts", () => {
    const onDismiss = vi.fn();
    render(
      <Toast onDismiss={onDismiss} duration={Number.POSITIVE_INFINITY} variant="success">
        <ToastTitle>Done</ToastTitle>
        <ToastDescription>All pages</ToastDescription>
        <ToastClose aria-label="Close toast" />
      </Toast>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Close toast" }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("throws a clear error outside <ToastProvider>", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(() => render(<ToastViewport />)).toThrow(/ToastProvider/);
    spy.mockRestore();
  });
});

describe("Progress", () => {
  it("is a determinate progressbar with its value", () => {
    render(<Progress value={40} aria-label="Compressing" />);
    const bar = screen.getByRole("progressbar", { name: "Compressing" });
    expect(bar.getAttribute("aria-valuenow")).toBe("40");
    expect(bar.getAttribute("aria-valuemin")).toBe("0");
    expect(bar.getAttribute("aria-valuemax")).toBe("100");
    const indicator = bar.querySelector('[data-slot="progress-indicator"]') as HTMLElement;
    expect(indicator.style.width).toBe("40%");
  });

  it("is indeterminate without a value", () => {
    render(<Progress value={null} aria-label="Working" />);
    const bar = screen.getByRole("progressbar", { name: "Working" });
    expect(bar.hasAttribute("aria-valuenow")).toBe(false);
    expect(bar.querySelector("[data-indeterminate]")).toBeTruthy();
  });

  it("offers the Golden Thread variant", () => {
    render(<Progress value={50} variant="golden" aria-label="Recipe" />);
    const bar = screen.getByRole("progressbar");
    expect(bar.getAttribute("data-variant")).toBe("golden");
    expect(bar.querySelector('[data-slot="progress-indicator"]')?.className).toContain("to-gold");
  });

  it("honours custom bounds", () => {
    render(<Progress value={3} min={0} max={4} aria-label="Steps" />);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuemax")).toBe("4");
  });
});

describe("Skeleton", () => {
  it("is decorative and respects reduced motion", () => {
    const { container } = render(<Skeleton className="h-4 w-20" />);
    const el = container.firstElementChild as HTMLElement;
    expect(el.getAttribute("aria-hidden")).toBe("true");
    expect(el.className).toContain("motion-reduce:animate-none");
    expect(el.className).toContain("animate-shimmer");
    expect(el.className).toContain("h-4");
  });

  it("offers a pulse variant", () => {
    const { container } = render(<Skeleton variant="pulse" />);
    const el = container.firstElementChild as HTMLElement;
    expect(el.className).toContain("animate-pulse");
    expect(el.className).toContain("motion-reduce:animate-none");
  });
});
