// A runtime whose model provider sends prompts only refuses a call with files
// as a catchable error, before anything is sent, rather than dropping them.
import llm from "submilli:llm";
import fs from "submilli:fs";

function main(): void {
  fs.writeText("/notes.md", "# Notes");
  let message = "";
  try {
    llm.text("claude-haiku-4-5", "Summarize.", ["/notes.md"]);
  } catch (e) {
    message = (e as Error).message;
  }
  assert(message.indexOf("does not accept files") >= 0, "the refusal says why: " + message);
  assert(llm.text("claude-haiku-4-5", "hi").ok, "a call without files still runs");
}
