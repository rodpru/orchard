export function withItem(set: Set<string>, item: string): Set<string> {
  return set.has(item) ? set : new Set(set).add(item);
}

export function withoutItem(set: Set<string>, item: string): Set<string> {
  if (!set.has(item)) return set;
  const next = new Set(set);
  next.delete(item);
  return next;
}

export function withoutKey<T>(record: Record<string, T>, key: string): Record<string, T> {
  if (!(key in record)) return record;
  const next = { ...record };
  delete next[key];
  return next;
}
