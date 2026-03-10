import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";

export default function Loading() {
  return (
    <div className="space-y-8">
      <div className="space-y-3">
        <Skeleton className="h-4 w-32 rounded-full" />
        <Skeleton className="h-16 w-full max-w-3xl rounded-3xl" />
        <Skeleton className="h-8 w-full max-w-2xl rounded-2xl" />
      </div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        {Array.from({ length: 4 }).map((_, index) => (
          <Card
            key={index}
            className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]"
          >
            <CardHeader className="space-y-2">
              <Skeleton className="h-3 w-24 rounded-full" />
              <Skeleton className="h-10 w-40 rounded-2xl" />
            </CardHeader>
            <CardContent>
              <Skeleton className="h-4 w-full rounded-full" />
            </CardContent>
          </Card>
        ))}
      </div>
      <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
        <CardContent className="space-y-4 pt-6">
          {Array.from({ length: 6 }).map((_, index) => (
            <Skeleton key={index} className="h-12 w-full rounded-2xl" />
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
