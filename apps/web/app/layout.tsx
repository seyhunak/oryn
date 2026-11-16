import type { Metadata } from "next";
import { DM_Mono, Instrument_Sans, Schibsted_Grotesk } from "next/font/google";
import "./globals.css";
import { Providers } from "./providers";
import { THEME_BOOT_SCRIPT } from "@/lib/theme-boot";

const display = Schibsted_Grotesk({ subsets: ["latin"], weight: ["700"], variable: "--font-schibsted", display: "swap" });
const body = Instrument_Sans({ subsets: ["latin"], weight: ["400", "500"], variable: "--font-instrument", display: "swap" });
const mono = DM_Mono({ subsets: ["latin"], weight: ["400", "500"], variable: "--font-dm-mono", display: "swap" });

export const metadata: Metadata = {
  title: { default: "ORYN", template: "%s · ORYN" },
  description: "Self-hosted AI quality engineering. Build. Verify. Deliver.",
  icons: { icon: "/icon.svg" },
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    // suppressHydrationWarning: the boot script may set data-theme on <html> before React hydrates.
    <html lang="en" className={`${display.variable} ${body.variable} ${mono.variable}`} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_BOOT_SCRIPT }} />
      </head>
      <body>
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
