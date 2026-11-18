// `files` sends files from the program's filesystem with the prompt. The
// runtime tells each one's kind from its bytes, not its name, and refuses a
// file that is not a PDF, an image or UTF-8 text before anything is sent.
// The fake answers with what it received, so the name, kind and size of each
// file are checked without it quoting the file.
import llm from "submilli:llm";
import fs from "submilli:fs";

interface Summary {
  title: string;
}

function value(r: llm.Completion<string>): string {
  return r.ok ? r.value : "";
}

function main(): void {
  fs.writeText("/report.pdf", "%PDF-1.7 not really a PDF");
  fs.writeText("/notes.md", "# Notes");
  fs.write("/chart.bin", new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]));

  const one = llm.text("claude-haiku-4-5", "Summarize.", ["/report.pdf", "/notes.md"]);
  assert(
    value(one) === "answer 0: report.pdf application/pdf 25, notes.md text/plain 7",
    "each file arrives with its name, its kind and its size: " + value(one),
  );

  // The kind comes from the bytes: a PNG signature is an image whatever the name.
  const image = llm.text("claude-haiku-4-5", "Describe.", ["/chart.bin"]);
  assert(value(image) === "answer 0: chart.bin image/png 9", "a PNG is an image: " + value(image));

  // In `texts` each prompt carries its own files, and a plain string carries none.
  const many = llm.texts("claude-haiku-4-5", [
    { prompt: "Summarize the report.", files: ["/report.pdf"] },
    { prompt: "Summarize the notes.", files: ["/notes.md"] },
    { prompt: "Compare them.", files: ["/report.pdf", "/notes.md"] },
  ]);
  assert(many.length === 3, "one answer per prompt");
  assert(value(many[0]) === "answer 0: report.pdf application/pdf 25", "the first prompt's file: " + value(many[0]));
  assert(value(many[1]) === "answer 1: notes.md text/plain 7", "the second prompt's file: " + value(many[1]));
  assert(
    value(many[2]) === "answer 2: report.pdf application/pdf 25, notes.md text/plain 7",
    "a file several prompts name reaches each of them: " + value(many[2]),
  );
  const mixed = llm.texts("claude-haiku-4-5", ["hi", { prompt: "Describe.", files: ["/chart.bin"] }]);
  assert(value(mixed[0]).indexOf("answer 0 for 2 bytes") === 0, "a string prompt sends no files: " + value(mixed[0]));
  assert(value(mixed[1]) === "answer 1: chart.bin image/png 9", "beside one that does: " + value(mixed[1]));

  // The typed forms take `files` after the prompt.
  const typed = llm.call<Summary>("claude-haiku-4-5", "Title?", ["/notes.md"]);
  assert(!typed.ok && typed.reason === "invalid-output", "the fake's prose is not a Summary");
  const typedBatch = llm.batch<Summary>("claude-haiku-4-5", [{ prompt: "Title?", files: ["/notes.md"] }]);
  assert(typedBatch.length === 1, "batch takes files per prompt too");
  assert(llm.texts("claude-haiku-4-5", []).length === 0, "an empty batch sends nothing");

  // Without `files`, a call is what it was before.
  assert(value(llm.text("claude-haiku-4-5", "hi")).indexOf("answer 0 for 2 bytes") === 0, "no files");

  // A file that is none of the kinds a model reads is refused before sending.
  fs.write("/blob", new Uint8Array([1, 0, 2]));
  let kind = "";
  try {
    llm.text("claude-haiku-4-5", "Read this.", ["/blob"]);
  } catch (e: TypeError) {
    kind = e.message;
  }
  assert(kind.indexOf("/blob is not a PDF") >= 0, "an unreadable kind is a TypeError: " + kind);

  // An empty file is nothing a model can read.
  fs.writeText("/empty.txt", "");
  let empty = "";
  try {
    llm.text("claude-haiku-4-5", "Read this.", ["/empty.txt"]);
  } catch (e: TypeError) {
    empty = e.message;
  }
  assert(empty.indexOf("or is empty") >= 0, "an empty file is a TypeError: " + empty);

  // A batch refuses a bad file before sending any prompt.
  let batchKind = "";
  try {
    llm.texts("claude-haiku-4-5", ["fine", { prompt: "Read this.", files: ["/blob"] }]);
  } catch (e: TypeError) {
    batchKind = e.message;
  }
  assert(batchKind.indexOf("/blob is not a PDF") >= 0, "a batch's bad file is a TypeError: " + batchKind);

  // A missing file fails as `fs.read` fails.
  let missing = false;
  try {
    llm.text("claude-haiku-4-5", "Read this.", ["/absent.pdf"]);
  } catch (e) {
    missing = true;
  }
  assert(missing, "a missing file throws");
}
