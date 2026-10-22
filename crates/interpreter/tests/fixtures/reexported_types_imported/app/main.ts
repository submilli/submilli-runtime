// The consumer imports the re-exported types by their public names.
import { search, titleOf, page, score, reveal, renamed, SearchResult, Page, Score, Revealed, Renamed } from "@test/lib";

function main(): void {
  const annotated: SearchResult = search("cats")[0];
  assert(annotated.title === "cats", "a value annotated with the imported type");
  const built: SearchResult = { title: "birds", url: "v" };
  assert(titleOf(built) === "birds", "a value built against the imported type");

  const p: Page<SearchResult> = page("fish");
  assert(p.items[0].title === "fish", "a generic re-exported interface");

  const s: Score = score(3);
  assert(s.label === "score 3", "a re-exported type alias");

  const r: Revealed = reveal("x");
  assert(r.secret === "x", "an interface re-exported under another name");

  const n: Renamed = renamed("z");
  assert(n.name === "z", "an entry-module interface exported under another name");
}
