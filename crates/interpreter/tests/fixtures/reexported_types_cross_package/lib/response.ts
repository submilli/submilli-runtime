export interface SearchResult {
  title: string;
  url: string;
}

export interface Page<T> {
  items: T[];
  next: string | undefined;
}

export type Score = { value: number; label: string };

export interface Hidden {
  secret: string;
}

export function normalize(body: string): SearchResult[] {
  return [{ title: body, url: `https://example.com/${body}` }];
}
