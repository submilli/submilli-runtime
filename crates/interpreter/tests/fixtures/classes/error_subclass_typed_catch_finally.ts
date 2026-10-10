// A typed-catch filter mismatch re-raises the original error — and the
// re-raise still runs this try's `finally` before the outer handler sees it.
class NotFoundError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "NotFoundError";
  }
}

class ConflictError extends Error {
  constructor(m: string) {
    super(m);
    this.name = "ConflictError";
  }
}

function main(): void {
  let trail = "";
  try {
    try {
      throw new NotFoundError("missing");
    } catch (e: ConflictError) {
      trail = trail + "bound!";
      assert(false, "sibling class must not bind");
    } finally {
      trail = trail + "finally;";
    }
  } catch (e) {
    trail = trail + "outer:" + e.name;
  }
  assert(trail === "finally;outer:NotFoundError", "finally runs before the re-raise reaches the outer catch");

  // Matching clause with finally: body then finally, no re-raise.
  let ok = "";
  try {
    try {
      throw new ConflictError("clash");
    } catch (e: ConflictError) {
      ok = ok + "caught:" + e.name + ";";
    } finally {
      ok = ok + "finally";
    }
  } catch (e) {
    assert(false, "matched clause must not re-raise");
  }
  assert(ok === "caught:ConflictError;finally", "match binds, body and finally run in order");
}
