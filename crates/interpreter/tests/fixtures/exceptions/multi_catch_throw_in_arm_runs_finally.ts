// A throw from inside a catch arm still runs this try's `finally`, and the
// outer handler sees the new error, not the original.
class InnerError extends Error {
  constructor() {
    super("inner");
    this.name = "InnerError";
  }
}

class OtherError extends Error {
  constructor() {
    super("other");
    this.name = "OtherError";
  }
}

function main(): void {
  let trail = "";
  try {
    try {
      throw new InnerError();
    } catch (e: OtherError) {
      trail = trail + "wrong;";
    } catch (e: InnerError) {
      trail = trail + "arm;";
      throw new Error("replaced");
    } finally {
      trail = trail + "fin;";
    }
  } catch (e) {
    trail = trail + "outer:" + e.message;
  }
  assert(trail === "arm;fin;outer:replaced", "finally runs before the re-thrown error escapes");
}
