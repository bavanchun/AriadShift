"use client";

import { FileTextIcon, MoonIcon, SunIcon } from "lucide-react";
import * as React from "react";
import { Alert, AlertDescription, AlertTitle } from "#components/alert";
import { Badge } from "#components/badge";
import { Button } from "#components/button";
import { Card, CardContent, CardHeader, CardTitle } from "#components/card";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "#components/command";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "#components/dialog";
import { DropOverlay } from "#components/drop-overlay";
import { Kbd, KbdGroup } from "#components/kbd";
import { Label } from "#components/label";
import { type PageRotation, PageThumbnail } from "#components/page-thumbnail";
import { Popover, PopoverContent, PopoverTrigger } from "#components/popover";
import { Progress } from "#components/progress";
import { Separator } from "#components/separator";
import { Skeleton } from "#components/skeleton";
import { Slider } from "#components/slider";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "#components/tabs";
import { ToastAction, ToastProvider, ToastViewport, useToast } from "#components/toast";
import { ToggleGroup, ToggleGroupItem } from "#components/toggle-group";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "#components/tooltip";
import { DirectionProvider } from "#lib/direction";
import { cn } from "#lib/utils";

export type ShowcaseTheme = "light" | "dark";
export type ShowcaseDirection = "ltr" | "rtl";

export interface UiDevShowcaseProps {
  defaultTheme?: ShowcaseTheme;
  defaultDirection?: ShowcaseDirection;
  className?: string;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  const id = React.useId();
  return (
    <section aria-labelledby={id} data-showcase-section={title} className="flex flex-col gap-3">
      <h2 id={id} className="text-sm font-semibold text-muted-foreground">
        {title}
      </h2>
      {children}
    </section>
  );
}

function ToastDemo() {
  const { toast, dismiss } = useToast();
  return (
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" onClick={() => toast({ title: "Merged 3 files", thread: true })}>
        Default toast
      </Button>
      <Button
        variant="outline"
        onClick={() =>
          toast({
            variant: "success",
            title: "Compressed to 1.2 MB",
            description: "Saved 64% of the original size.",
            action: <ToastAction>Undo</ToastAction>,
          })
        }
      >
        Success toast
      </Button>
      <Button
        variant="outline"
        onClick={() =>
          toast({ variant: "warning", title: "Scanned pages found", description: "Run OCR first." })
        }
      >
        Warning toast
      </Button>
      <Button
        variant="outline"
        onClick={() =>
          toast({
            variant: "destructive",
            title: "Password required",
            description: "This PDF is locked.",
            duration: Number.POSITIVE_INFINITY,
          })
        }
      >
        Destructive toast
      </Button>
      <Button variant="ghost" onClick={() => dismiss()}>
        Clear
      </Button>
    </div>
  );
}

function ShowcaseBody({ container }: { container: HTMLElement | null }) {
  const [rotation, setRotation] = React.useState<PageRotation>(0);
  const [dragOver, setDragOver] = React.useState(false);
  const [paletteOpen, setPaletteOpen] = React.useState(false);
  const [volume, setVolume] = React.useState(40);
  const [lastRun, setLastRun] = React.useState("none");

  return (
    <div className="grid gap-8">
      <Section title="Buttons, badges and keys">
        <div className="flex flex-wrap items-center gap-2">
          <Button>Primary</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="outline">Outline</Button>
          <Button variant="destructive">Destructive</Button>
          <Badge>Browser</Badge>
          <Badge variant="secondary">Desktop</Badge>
          <KbdGroup keys={["mod", "K"]} />
          <KbdGroup keys={["shift", "alt", "P"]} />
          <Kbd>Esc</Kbd>
        </div>
      </Section>

      <Section title="Dialog, popover and tooltip">
        <div className="flex flex-wrap items-center gap-2">
          <Dialog>
            <DialogTrigger render={<Button variant="outline" />}>Open dialog</DialogTrigger>
            <DialogContent container={container}>
              <DialogHeader>
                <DialogTitle>Remove 3 pages?</DialogTitle>
                <DialogDescription>You can undo this until you close the tab.</DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
                <DialogClose render={<Button variant="destructive" />}>Remove</DialogClose>
              </DialogFooter>
            </DialogContent>
          </Dialog>
          <Popover>
            <PopoverTrigger render={<Button variant="outline" />}>Open popover</PopoverTrigger>
            <PopoverContent container={container}>
              <p className="font-medium">Runs in your browser</p>
              <p className="text-muted-foreground">No bytes leave this device.</p>
            </PopoverContent>
          </Popover>
          <Tooltip>
            <TooltipTrigger render={<Button variant="outline" />}>Hover or focus me</TooltipTrigger>
            <TooltipContent container={container}>Rotate every page</TooltipContent>
          </Tooltip>
        </div>
      </Section>

      <Section title="Tabs and toggle group">
        <div className="grid gap-4 sm:grid-cols-2">
          <Tabs defaultValue="merge">
            <TabsList aria-label="Tools">
              <TabsTrigger value="merge">Merge</TabsTrigger>
              <TabsTrigger value="split">Split</TabsTrigger>
              <TabsTrigger value="compress">Compress</TabsTrigger>
            </TabsList>
            <TabsContent value="merge">Combine files into one PDF.</TabsContent>
            <TabsContent value="split">Cut a PDF at page numbers.</TabsContent>
            <TabsContent value="compress">Shrink the file size.</TabsContent>
          </Tabs>
          <div className="flex flex-col gap-3">
            <ToggleGroup defaultValue={["fit"]} aria-label="Zoom">
              <ToggleGroupItem value="fit">Fit</ToggleGroupItem>
              <ToggleGroupItem value="width">Width</ToggleGroupItem>
              <ToggleGroupItem value="100">100%</ToggleGroupItem>
            </ToggleGroup>
            <ToggleGroup multiple variant="outline" defaultValue={["b"]} aria-label="Format">
              <ToggleGroupItem value="b">Bold</ToggleGroupItem>
              <ToggleGroupItem value="i">Italic</ToggleGroupItem>
              <ToggleGroupItem value="u">Underline</ToggleGroupItem>
            </ToggleGroup>
          </div>
        </div>
      </Section>

      <Section title="Slider and progress">
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="grid gap-2">
            <Label>Image quality</Label>
            <Slider
              value={volume}
              onValueChange={(next) => setVolume(next as number)}
              showValue
              thumbLabel="Image quality"
            />
            <Slider defaultValue={[20, 70]} thumbLabel={["Range start", "Range end"]} />
          </div>
          <div className="grid gap-3">
            <Progress value={64} aria-label="Compressing" />
            <Progress value={64} variant="golden" aria-label="Recipe progress" />
            <Progress value={null} aria-label="Working" />
          </div>
        </div>
      </Section>

      <Section title="Command palette">
        <div className="grid gap-3 sm:grid-cols-2">
          <Command className="border" loop>
            <CommandInput placeholder="Type a command or describe a recipe" />
            <CommandList>
              <CommandEmpty>No matching command.</CommandEmpty>
              <CommandGroup heading="Tools">
                <CommandItem keywords={["combine"]} onSelect={setLastRun}>
                  <FileTextIcon aria-hidden="true" />
                  Merge PDFs
                  <CommandShortcut>
                    <KbdGroup keys={["mod", "M"]} />
                  </CommandShortcut>
                </CommandItem>
                <CommandItem onSelect={setLastRun}>Split PDF</CommandItem>
                <CommandItem disabled>Sign (needs desktop)</CommandItem>
              </CommandGroup>
              <CommandSeparator />
              <CommandGroup heading="Recipes">
                <CommandItem onSelect={setLastRun}>Scan to searchable PDF</CommandItem>
              </CommandGroup>
            </CommandList>
          </Command>
          <div className="flex flex-col items-start gap-2">
            <Button variant="outline" onClick={() => setPaletteOpen(true)}>
              Open palette dialog
            </Button>
            <p className="text-sm text-muted-foreground">Last command: {lastRun}</p>
            <CommandDialog open={paletteOpen} onOpenChange={setPaletteOpen} container={container}>
              <CommandInput placeholder="Search" />
              <CommandList>
                <CommandEmpty>No matching command.</CommandEmpty>
                <CommandItem onSelect={() => setPaletteOpen(false)}>Compress PDF</CommandItem>
                <CommandItem onSelect={() => setPaletteOpen(false)}>Rotate pages</CommandItem>
              </CommandList>
            </CommandDialog>
          </div>
        </div>
      </Section>

      <Section title="Toast">
        <ToastDemo />
      </Section>

      <Section title="Skeleton">
        <div className="grid max-w-sm gap-2" aria-busy="true">
          <Skeleton className="h-4 w-3/4" />
          <Skeleton className="h-4 w-1/2" variant="pulse" />
          <Skeleton className="h-24 w-full" />
        </div>
      </Section>

      <Section title="Page thumbnails">
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <PageThumbnail
            page={1}
            rotation={rotation}
            onRotate={() => setRotation(((rotation + 90) % 360) as PageRotation)}
            onDelete={() => undefined}
            dragHandleProps={{}}
          >
            <span className="text-xs text-neutral-600">Page 1</span>
          </PageThumbnail>
          <PageThumbnail page={2} defaultSelected onRotate={() => undefined} blank>
            <span className="text-xs text-neutral-600">Page 2</span>
          </PageThumbnail>
          <PageThumbnail page={3} scanned dragHandleProps={{}}>
            <span className="text-xs text-neutral-600">Page 3</span>
          </PageThumbnail>
          <PageThumbnail page={4} dragging dragHandleProps={{}}>
            <span className="text-xs text-neutral-600">Page 4</span>
          </PageThumbnail>
        </div>
      </Section>

      <Section title="Drop overlay">
        <Card
          className="relative min-h-56"
          onDragEnter={() => setDragOver(true)}
          onDragLeave={() => setDragOver(false)}
          onDrop={() => setDragOver(false)}
        >
          <CardHeader>
            <CardTitle>Drop target</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col items-start gap-2">
            <p className="text-sm text-muted-foreground">
              Drag a file over this card, or force the overlay on.
            </p>
            <Button variant="outline" onClick={() => setDragOver((current) => !current)}>
              Toggle overlay
            </Button>
          </CardContent>
          <DropOverlay
            active={dragOver}
            description="Release to open it on this device."
            hints={["PDF", "DOCX", "Images", "Merge", "Compress"]}
          />
        </Card>
      </Section>

      <Section title="Status">
        <Alert>
          <AlertTitle>Honest runtime</AlertTitle>
          <AlertDescription>Every tool states whether it runs in the browser.</AlertDescription>
        </Alert>
      </Section>
    </div>
  );
}

/**
 * Storybook-free visual harness for every `@ariadshift/ui` component. Theme and direction
 * are local to the showcase (it never touches `<html>`), and floating panels portal into the
 * showcase root so they follow the chosen theme and direction.
 */
export function UiDevShowcase({
  defaultTheme = "light",
  defaultDirection = "ltr",
  className,
}: UiDevShowcaseProps) {
  const [theme, setTheme] = React.useState<ShowcaseTheme>(defaultTheme);
  const [direction, setDirection] = React.useState<ShowcaseDirection>(defaultDirection);
  const [root, setRoot] = React.useState<HTMLDivElement | null>(null);

  return (
    <DirectionProvider direction={direction}>
      <TooltipProvider>
        <ToastProvider>
          <div
            ref={setRoot}
            dir={direction}
            data-slot="ui-dev-showcase"
            data-theme={theme}
            className={cn(
              "min-h-dvh bg-background p-6 text-foreground",
              theme === "dark" && "dark",
              className,
            )}
          >
            <header className="mb-6 flex flex-wrap items-center justify-between gap-3">
              <h1 className="text-xl font-semibold">UI components</h1>
              <div className="flex flex-wrap items-center gap-3">
                <ToggleGroup
                  aria-label="Theme"
                  variant="outline"
                  value={[theme]}
                  onValueChange={(next) => next[0] && setTheme(next[0] as ShowcaseTheme)}
                >
                  <ToggleGroupItem value="light">
                    <SunIcon aria-hidden="true" />
                    Light
                  </ToggleGroupItem>
                  <ToggleGroupItem value="dark">
                    <MoonIcon aria-hidden="true" />
                    Dark
                  </ToggleGroupItem>
                </ToggleGroup>
                <ToggleGroup
                  aria-label="Direction"
                  variant="outline"
                  value={[direction]}
                  onValueChange={(next) => next[0] && setDirection(next[0] as ShowcaseDirection)}
                >
                  <ToggleGroupItem value="ltr">LTR</ToggleGroupItem>
                  <ToggleGroupItem value="rtl">RTL</ToggleGroupItem>
                </ToggleGroup>
              </div>
            </header>
            <Separator className="mb-6" />
            <ShowcaseBody container={root} />
            <ToastViewport />
          </div>
        </ToastProvider>
      </TooltipProvider>
    </DirectionProvider>
  );
}
