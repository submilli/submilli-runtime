import { show_song_library, search_songs } from "@test/music";
function main(): void {
  const songs: ReturnType<typeof show_song_library> = [];
  songs.push(...show_song_library());
  const hits = [] as ReturnType<typeof search_songs>;
  hits.push(...search_songs("two", 5));
  const query: Parameters<typeof search_songs> = ["three"];
  assert(songs[0].title === "one" && hits[0].score === 5 && query[0] === "three");
}
