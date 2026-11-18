// deny-capability: fs.read
// Each file is a read of the program's filesystem and is gated as `fs.read`,
// so a program that may call the model but not read a file cannot send it.
import llm from "submilli:llm";
import fs from "submilli:fs";

function main(): void {
  fs.writeText("/secret.txt", "do not send");
  let capability = "";
  try {
    llm.text("claude-haiku-4-5", "Summarize.", ["/secret.txt"]);
    assert(false, "a file the program may not read is not sent");
  } catch (e: PermissionDeniedError) {
    capability = e.capability;
  }
  assert(capability === "fs.read", "the refusal names fs.read: " + capability);

  // A batch's files are gated before any of its prompts is sent.
  let gated = false;
  try {
    llm.texts("claude-haiku-4-5", ["hi", { prompt: "Summarize.", files: ["/secret.txt"] }]);
  } catch (e: PermissionDeniedError) {
    gated = true;
  }
  assert(gated, "a batch's files are gated too");

  // Without files, the model call itself is still allowed.
  const plain = llm.text("claude-haiku-4-5", "hi");
  assert(plain.ok, "a call without files does not read");
}
