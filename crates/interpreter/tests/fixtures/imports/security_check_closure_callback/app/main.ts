// deny-capability: test.com/op
import { relay, runCallback } from "@test/outer";
import { guarded } from "@test/inner";

function main(): void {
  // The closure body is the script's code, so the inner check names main even
  // though the frame that invoked it belongs to @test/outer.
  let viaCallback = "";
  runCallback((n: number): void => {
    try {
      guarded(n);
    } catch (e: PermissionDeniedError) {
      viaCallback = e.caller;
    }
  });
  assert(viaCallback === "main", "a callback keeps its author's identity");

  // Contrast: the same inner check reached through @test/outer's own code.
  let viaRelay = "";
  try {
    relay(1);
  } catch (e: PermissionDeniedError) {
    viaRelay = e.caller;
  }
  assert(viaRelay === "@test/outer", "package code answers the package");
}
