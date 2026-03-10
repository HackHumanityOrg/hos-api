import Link from "next/link";

import {
  Avatar,
  AvatarFallback,
} from "@/components/ui/avatar";
import { accountInitials, truncateMiddle } from "@/lib/format";

export function AccountLink({
  accountId,
  compact = false,
}: {
  accountId: string;
  compact?: boolean;
}) {
  const href = `/accounts/${encodeURIComponent(accountId)}`;

  return (
    <Link
      href={href}
      className="inline-flex items-center gap-2 text-sm font-medium text-foreground transition-colors hover:text-primary"
    >
      <Avatar size={compact ? "sm" : "default"}>
        <AvatarFallback>{accountInitials(accountId)}</AvatarFallback>
      </Avatar>
      <span className="max-w-[18ch] truncate" title={accountId}>
        {compact ? truncateMiddle(accountId, 5) : accountId}
      </span>
    </Link>
  );
}
