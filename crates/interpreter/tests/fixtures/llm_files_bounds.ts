// The files sent with one prompt are bounded together, before anything is
// sent: here the runtime allows 16 bytes of files per prompt. Each prompt is
// its own request, so a batch whose prompts each fit is not refused for their
// sum. The refusal is a catchable RangeError that counts the bytes and never
// quotes them.
import llm from "submilli:llm";
import fs from "submilli:fs";

function main(): void {
  fs.writeText("/a.txt", "0123456789");
  fs.writeText("/b.txt", "abcdefghij");

  assert(llm.text("claude-haiku-4-5", "One.", ["/a.txt"]).ok, "10 bytes fit");

  let caught = "";
  try {
    llm.text("claude-haiku-4-5", "Both.", ["/a.txt", "/b.txt"]);
  } catch (e: RangeError) {
    caught = e.message;
  }
  assert(caught.indexOf("bytes of files sent with one prompt") >= 0, "the refusal names the bound: " + caught);
  assert(caught.indexOf("20") >= 0 && caught.indexOf("16") >= 0, "and counts the bytes: " + caught);
  assert(caught.indexOf("abcdefghij") < 0, "and never quotes a file");

  const apart = llm.texts("claude-haiku-4-5", [
    { prompt: "One.", files: ["/a.txt"] },
    { prompt: "Other.", files: ["/b.txt"] },
  ]);
  assert(apart.length === 2 && apart[0].ok && apart[1].ok, "10 bytes per prompt fit, 20 in all");

  let together = "";
  try {
    llm.texts("claude-haiku-4-5", ["fine", { prompt: "Both.", files: ["/a.txt", "/b.txt"] }]);
  } catch (e: RangeError) {
    together = e.message;
  }
  assert(together.indexOf("20") >= 0, "one prompt over the bound refuses the batch: " + together);
}
