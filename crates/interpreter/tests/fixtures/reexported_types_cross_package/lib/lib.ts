import { normalize, Page, Score, Hidden } from "./response";
export { SearchResult, Page } from "./response";
export type { Score } from "./response";
export { Hidden as Revealed } from "./response";
import type { SearchResult } from "./response";

interface Local {
  name: string;
}

export { Local as Renamed };

export function search(q: string): SearchResult[] {
  return normalize(q);
}

export function titleOf(result: SearchResult): string {
  return result.title;
}

export function page(q: string): Page<SearchResult> {
  return { items: normalize(q), next: undefined };
}

export function score(value: number): Score {
  return { value, label: `score ${value}` };
}

export function reveal(secret: string): Hidden {
  return { secret };
}

export function renamed(name: string): Local {
  return { name };
}
