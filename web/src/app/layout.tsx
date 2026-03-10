import type { Metadata } from "next";
import Link from "next/link";
import { ArrowUpRight, Orbit } from "lucide-react";
import { JetBrains_Mono, Space_Grotesk } from "next/font/google";

import { buttonVariants } from "@/components/ui/button-styles";
import { SiteNav } from "@/components/venear/site-nav";
import "./globals.css";

const spaceGrotesk = Space_Grotesk({
  variable: "--font-space-grotesk",
  subsets: ["latin"],
});

const jetbrainsMono = JetBrains_Mono({
  variable: "--font-jetbrains-mono",
  subsets: ["latin"],
});

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
      <body
        className={`${spaceGrotesk.variable} ${jetbrainsMono.variable} antialiased`}
      >
        <div className="pointer-events-none fixed inset-0 overflow-hidden">
          <div className="absolute inset-x-0 top-[-16rem] h-[32rem] bg-[radial-gradient(circle_at_top,rgba(40,138,145,0.24),transparent_65%)]" />
          <div className="absolute right-[-12rem] top-[20%] h-[28rem] w-[28rem] rounded-full bg-[radial-gradient(circle,rgba(238,121,95,0.2),transparent_68%)]" />
          <div className="absolute bottom-[-12rem] left-[-8rem] h-[24rem] w-[24rem] rounded-full bg-[radial-gradient(circle,rgba(225,181,80,0.18),transparent_72%)]" />
        </div>
        <div className="relative min-h-screen">
          <header className="sticky top-0 z-40 border-b border-white/50 bg-[color:rgba(250,247,239,0.78)] backdrop-blur-xl">
            <div className="mx-auto flex max-w-7xl flex-col gap-4 px-4 py-4 sm:px-6 lg:flex-row lg:items-center lg:justify-between lg:px-8">
              <div className="flex items-center gap-3">
                <div className="flex size-11 items-center justify-center rounded-2xl bg-[color:var(--color-primary)] text-primary-foreground shadow-[0_12px_30px_rgba(40,138,145,0.28)]">
                  <Orbit className="size-5" />
                </div>
                <div>
                  <Link
                    href="/"
                    className="text-lg font-semibold tracking-[-0.03em] text-foreground"
                  >
                    veNEAR Observatory
                  </Link>
                  <p className="text-sm text-muted-foreground">
                    Canonical governance data, exposed for apps instead of compatibility shims.
                  </p>
                </div>
              </div>
              <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
                <SiteNav />
                <Link
                  href="/activity"
                  className={buttonVariants({ variant: "outline", size: "sm" })}
                >
                  Activity pulse
                  <ArrowUpRight className="size-4" />
                </Link>
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
