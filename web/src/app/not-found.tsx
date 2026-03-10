import Link from "next/link";

import { buttonVariants } from "@/components/ui/button-styles";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export default function NotFound() {
  return (
    <div className="flex min-h-[60vh] items-center justify-center">
      <Card className="w-full max-w-xl border-white/60 bg-white/82 text-center shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
        <CardHeader>
          <CardTitle className="text-3xl tracking-[-0.04em]">
            The requested veNEAR resource was not found.
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm leading-6 text-muted-foreground">
            The API returned `404`, or the route parameters do not describe a valid proposal or account.
          </p>
          <div className="flex justify-center gap-2">
            <Link href="/proposals" className={buttonVariants()}>
              Browse proposals
            </Link>
            <Link
              href="/accounts"
              className={buttonVariants({ variant: "outline" })}
            >
              Browse accounts
            </Link>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
