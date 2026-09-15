// Deliberately no `// deny-capability:` directive. The fixture runtime uses the
// default allow-all policy, which is exactly the claim: `main` cannot read a
// secret even when nothing is configured to stop it.
//
// This fixture lives at the fixtures root on purpose. CI runs plain
// `cargo test --workspace`, whose default suite keeps every top-level fixture
// but only the first fixture in each subdirectory — nested, this would not run.
import { get } from "submilli:secrets";

function main(): void {
  let caught = false;
  try {
    get("ANY_SECRET");
  } catch (e: PermissionDeniedError) {
    caught = true;
    assert(e.capability === "secrets.get", "capability field");
    assert(e.caller === "main", "caller field");
    assert(e.name === "PermissionDeniedError", "name field");

    // Not a policy decision, and the message must not say it is: an operator
    // cannot change this outcome, so pointing at the policy would send the
    // model looking for a rule change that cannot exist.
    assert(
      !e.message.includes("operator's policy"),
      "the refusal is not attributed to the policy"
    );
    assert(
      e.message.includes("no policy can grant this"),
      "the refusal says it is not configurable"
    );

    // The fix, and the misreading it forecloses.
    assert(
      e.message.includes("pass the secret NAME to that package's API"),
      "the refusal names the fix"
    );
    assert(
      e.message.includes("resolves it internally and never returns it"),
      "the refusal forecloses routing the value back through a package"
    );

    // The do-not-work-around force, kept from the policy denial.
    assert(
      e.message.includes("Do not work around this denial"),
      "the refusal keeps the do-not-work-around sentence"
    );
    assert(e.message.includes("Report it and stop."), "the refusal ends the same way");
  }
  assert(caught, "main reading a secret must throw");

  // The denial for an undeclared name is byte-identical, so `main` cannot use
  // the error to learn which secrets the blueprint declares.
  let first = "";
  let second = "";
  try {
    get("DECLARED_LOOKING_NAME");
  } catch (e: PermissionDeniedError) {
    first = e.message;
  }
  try {
    get("SOMETHING_ELSE_ENTIRELY");
  } catch (e: PermissionDeniedError) {
    second = e.message;
  }
  assert(first === second, "the denial cannot distinguish secret names");
  assert(first !== "", "both reads must have thrown");
}
