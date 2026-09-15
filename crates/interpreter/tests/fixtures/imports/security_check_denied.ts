// deny-capability: test.com/op
// deny-capability: fs.write
import { check } from "submilli:security";
import { writeText } from "submilli:fs";

function main(): void {
  // An explicit check denial throws a catchable PermissionDeniedError with
  // the structured fields populated.
  let caught = false;
  try {
    check("test.com/op", { foo: 1 });
  } catch (e: PermissionDeniedError) {
    caught = true;
    assert(e.capability === "test.com/op", "capability field");
    assert(e.caller === "main", "caller field");
    assert(e.reason === "denied by fixture policy", "reason field");
    assert(e.name === "PermissionDeniedError", "name field");
    assert(
      e.message.includes(
        "permission denied: caller=main capability=test.com/op: denied by fixture policy"
      ),
      "message keeps the trap format"
    );
    assert(e instanceof Error, "subclass of Error");
  }
  assert(caught, "denied check must throw");

  // catch (e: Error) also matches — subclass relation.
  let viaError = "";
  try {
    check("test.com/op", { foo: 2 });
  } catch (e: Error) {
    viaError = e.name;
  }
  assert(viaError === "PermissionDeniedError", "Error arm catches the subclass");

  // Capabilities outside the deny list still pass.
  check("test.com/other", { foo: 3 });

  // A gated stdlib op denies through the same path.
  let fsCap = "";
  try {
    writeText("/x.txt", "hi");
  } catch (e: PermissionDeniedError) {
    fsCap = e.capability + ":" + e.caller;
  }
  assert(fsCap === "fs.write:main", "fs op throws PermissionDeniedError");
}
