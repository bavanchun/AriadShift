import type { Metadata } from "next";
import { Suspense } from "react";
import { ConverterApp } from "@/components/converter/converter-app";

export const metadata: Metadata = {
  title: "Converter",
  description:
    "Convert Markdown, HTML, DOCX, EPUB, digital PDF and AriadShift IR JSON in your browser. Files stay on your device.",
  alternates: { canonical: "/app" },
};

export default function ConverterPage() {
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <h1 className="text-3xl font-bold tracking-tight">Convert a document</h1>
        <p className="text-muted-foreground">
          Choose a file, pick a target and download the result. Everything happens in this tab.
        </p>
      </div>
      <Suspense fallback={null}>
        <ConverterApp />
      </Suspense>
    </div>
  );
}
