export type SearchParamsRecord = Record<string, string | string[] | undefined>;

type HrefValue = string | number | boolean | null | undefined;

export function readSearchParam(
  searchParams: SearchParamsRecord,
  key: string,
): string | undefined {
  const value = searchParams[key];
  return Array.isArray(value) ? value[0] : value;
}

export function buildHref(
  pathname: string,
  searchParams: SearchParamsRecord,
  overrides: Record<string, HrefValue>,
): string {
  const next = new URLSearchParams();

  for (const [key, value] of Object.entries(searchParams)) {
    if (Array.isArray(value)) {
      for (const item of value) {
        next.append(key, item);
      }
      continue;
    }

    if (value) {
      next.set(key, value);
    }
  }

  for (const [key, value] of Object.entries(overrides)) {
    if (value === null || value === undefined || value === "") {
      next.delete(key);
      continue;
    }

    next.set(key, String(value));
  }

  const query = next.toString();
  return query ? `${pathname}?${query}` : pathname;
}
