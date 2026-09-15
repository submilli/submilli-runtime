// deny-capability: test.com/op
import { relay } from "@test/outer";
import { guarded } from "@test/inner";

function main(): void {
  // `check` names the immediate invoker of the code that runs it. Reached
  // through @test/outer, the inner package's check answers @test/outer — not
  // main, which is further out, and not @test/inner, which is the running code.
  let viaRelay = "";
  try {
    relay(1);
  } catch (e: PermissionDeniedError) {
    viaRelay = e.caller;
  }
  assert(viaRelay === "@test/outer", "chain answers the middle package");

  // The same check called straight from the script answers main.
  let direct = "";
  try {
    guarded(2);
  } catch (e: PermissionDeniedError) {
    direct = e.caller;
  }
  assert(direct === "main", "direct call answers main");
}
