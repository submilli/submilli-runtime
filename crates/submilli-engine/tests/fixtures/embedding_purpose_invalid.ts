// expect-error: "document"
// `purpose` is `"query" | "document"`: any other string is a compile error that
// names the accepted values, so a misspelled purpose never reaches a provider.
// (The host re-validates at run time as a backstop; the checked cast the
// language puts around a literal type means no program can reach that path.)
import embedding from "submilli:embedding";

function main(): void {
  embedding.embed("fixture-embedding", ["a"], "clustering");
}
