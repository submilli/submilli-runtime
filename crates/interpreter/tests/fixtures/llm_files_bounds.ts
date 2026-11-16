// The files of one call are bounded together, before anything is sent: here
// the runtime allows 16 bytes of files per call. The refusal is a catchable
// RangeError that counts the bytes and never quotes them.
import llm from "submilli:llm";
import fs from "submilli:fs";

function main(): void {
  fs.writeText("/a.txt", "0123456789");
  fs.writeText("/b.txt", "abcdefghij");

  assert(llm.text("claude-haiku-4-5", "One.", undefined, ["/a.txt"]).ok, "10 bytes fit");

  let caught = "";
  try {
    llm.text("claude-haiku-4-5", "Both.", undefined, ["/a.txt", "/b.txt"]);
  } catch (e: RangeError) {
    caught = e.message;
  }
  assert(caught.indexOf("bytes of files in one call") >= 0, "the refusal names the bound: " + caught);
  assert(caught.indexOf("20") >= 0 && caught.indexOf("16") >= 0, "and counts the bytes: " + caught);
  assert(caught.indexOf("abcdefghij") < 0, "and never quotes a file");
}
