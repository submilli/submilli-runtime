// The types are named only through the namespace import: field reads on the
// functions' results must resolve without the consumer importing the type.
import { search, titleOf, page, score, reveal, renamed } from "@test/lib";
import * as lib from "@test/lib";

function main(): void {
  const results = search("cats");
  assert(results[0].title === "cats", "a field of a returned re-exported interface");
  assert(results[0].url === "https://example.com/cats", "a second field");
  assert(titleOf({ title: "dogs", url: "u" }) === "dogs", "a re-exported interface as a parameter");
  assert(titleOf(results[0]) === "cats", "the returned value passed back");

  const annotated: lib.SearchResult = results[0];
  assert(annotated.title === "cats", "a value annotated with the namespace's type");
  const built: lib.SearchResult = { title: "birds", url: "v" };
  assert(titleOf(built) === "birds", "a value built against the namespace's type");
  assert(lib.search("ns")[0].title === "ns", "a namespace call's result");

  const p = page("fish");
  assert(p.items[0].title === "fish", "a field of a generic re-exported interface");
  assert(p.next === undefined, "an optional field of the generic");
  const typed: lib.Page<lib.SearchResult> = p;
  assert(typed.items[0].url === "https://example.com/fish", "the generic named through the namespace");

  const s = score(3);
  assert(s.label === "score 3" && s.value === 3, "a re-exported type alias");
  const alias: lib.Score = s;
  assert(alias.value === 3, "the alias named through the namespace");

  const r = reveal("x");
  assert(r.secret === "x", "an interface re-exported under another name");
  const revealed: lib.Revealed = r;
  assert(revealed.secret === "x", "the renamed interface named through the namespace");

  const n = renamed("z");
  assert(n.name === "z", "an entry-module interface exported under another name");
  const named: lib.Renamed = n;
  assert(named.name === "z", "the renamed entry-module interface through the namespace");
}
