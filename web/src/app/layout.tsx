import type { Metadata } from "next";
import Link from "next/link";
import { Orbit } from "lucide-react";

import { SiteNav } from "@/components/venear/site-nav";
import "./globals.css";

export const metadata: Metadata = {
  title: {
    default: "veNEAR Observatory",
    template: "%s | veNEAR Observatory",
  },
  description:
    "An app-first Next.js explorer for the canonical veNEAR governance API.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body className="antialiased">
        <div className="relative min-h-screen">
          <header className="sticky top-0 z-40 border-b bg-background">
            <div className="mx-auto flex max-w-7xl flex-col gap-4 px-4 py-4 sm:px-6 lg:flex-row lg:items-center lg:justify-between lg:px-8">
              <div className="flex items-center gap-3">
                <div className="flex size-11 items-center justify-center rounded-lg border bg-primary text-primary-foreground">
                  <Orbit className="size-5" />
                </div>
                <div>
                  <Link href="/" className="text-lg font-semibold text-foreground">
                    veNEAR Observatory
                  </Link>
                  <p className="text-sm text-muted-foreground">
                    Canonical governance data, exposed for apps instead of compatibility shims.
                  </p>
                </div>
              </div>
              <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
                <SiteNav />
              </div>
            </div>
          </header>
          <main className="mx-auto max-w-7xl px-4 py-10 sm:px-6 lg:px-8">
            {children}
          </main>
          <footer className="mx-auto max-w-7xl px-4 pb-10 pt-2 text-sm text-muted-foreground sm:px-6 lg:px-8">
            Built with Next.js App Router and shadcn/ui against `/api/v1/venear`.
          </footer>
        </div>
      </body>
    </html>
  );
}
