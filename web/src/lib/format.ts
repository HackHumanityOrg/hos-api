import type { JsonValue, VenearTokenAmountDto } from "@/lib/venear-api";

const dateTimeFormatter = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  year: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

const relativeTimeFormatter = new Intl.RelativeTimeFormat("en", {
  numeric: "auto",
});

function withThousandsSeparators(value: string): string {
  return value.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

export function formatDecimal(value: string, maxFractionDigits = 2): string {
  const negative = value.startsWith("-");
  const normalized = negative ? value.slice(1) : value;
  const [integerPartRaw, fractionPartRaw = ""] = normalized.split(".");
  const integerPart = integerPartRaw.replace(/^0+(?=\d)/, "") || "0";
  const trimmedFraction = fractionPartRaw
    .slice(0, maxFractionDigits)
    .replace(/0+$/, "");
  const formattedInteger = withThousandsSeparators(integerPart);
  const prefix = negative ? "-" : "";

  return trimmedFraction
    ? `${prefix}${formattedInteger}.${trimmedFraction}`
    : `${prefix}${formattedInteger}`;
}

export function formatTokenAmount(
  amount: VenearTokenAmountDto | null | undefined,
  maxFractionDigits = 2,
): string {
  if (!amount) {
    return "—";
  }

  return `${formatDecimal(amount.decimal, maxFractionDigits)} ${amount.symbol}`;
}

export function formatTimestamp(value: string | null | undefined): string {
  if (!value) {
    return "—";
  }

  return dateTimeFormatter.format(new Date(value));
}

export function formatRelativeTime(value: string | null | undefined): string {
  if (!value) {
    return "—";
  }

  const now = Date.now();
  const then = new Date(value).getTime();
  const deltaSeconds = Math.round((then - now) / 1000);
  const absSeconds = Math.abs(deltaSeconds);

  if (absSeconds < 60) {
    return relativeTimeFormatter.format(deltaSeconds, "second");
  }

  const deltaMinutes = Math.round(deltaSeconds / 60);
  if (Math.abs(deltaMinutes) < 60) {
    return relativeTimeFormatter.format(deltaMinutes, "minute");
  }

  const deltaHours = Math.round(deltaMinutes / 60);
  if (Math.abs(deltaHours) < 24) {
    return relativeTimeFormatter.format(deltaHours, "hour");
  }

  const deltaDays = Math.round(deltaHours / 24);
  return relativeTimeFormatter.format(deltaDays, "day");
}

export function humanizeSnakeCase(value: string | null | undefined): string {
  if (!value) {
    return "Unknown";
  }

  return value
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export function truncateMiddle(value: string, visible = 6): string {
  if (value.length <= visible * 2 + 3) {
    return value;
  }

  return `${value.slice(0, visible)}...${value.slice(-visible)}`;
}

export function accountInitials(accountId: string): string {
  return accountId
    .replace(".near", "")
    .split(/[\W_]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("")
    .slice(0, 2);
}

export function percentageFromRaw(partRaw: string, totalRaw: string): string {
  try {
    const zero = BigInt(0);
    const ten = BigInt(10);
    const thousand = BigInt(1000);
    const part = BigInt(partRaw);
    const total = BigInt(totalRaw);

    if (total <= zero) {
      return "0%";
    }

    const basisPoints = (part * thousand) / total;
    const whole = basisPoints / ten;
    const tenths = basisPoints % ten;

    return tenths === zero ? `${whole}%` : `${whole}.${tenths}%`;
  } catch {
    return "0%";
  }
}

export function prettyJson(value: JsonValue): string {
  return JSON.stringify(value, null, 2);
}
