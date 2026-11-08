export interface Song { id: number; title: string }
export function show_song_library(): Song[] { return [{ id: 1, title: "one" }]; }
export function search_songs(query: string, limit?: number): { song: Song; score: number }[] {
  return [{ song: { id: 2, title: query }, score: limit ?? 1 }];
}
