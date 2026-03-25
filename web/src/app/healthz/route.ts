import { NextResponse } from "next/server";

export function GET() {
  return new NextResponse("ok", {
    status: 200,
    headers: {
      "cache-control": "no-store, max-age=0",
      "content-type": "text/plain; charset=utf-8",
    },
  });
}
