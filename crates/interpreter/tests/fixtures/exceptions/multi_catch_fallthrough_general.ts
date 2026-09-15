// An error matching no specific arm falls through to the untyped catch-all —
// including a same-shape sibling class, which shape-based dispatch would have
// mis-bound.
class HttpError extends Error {
  code: number;
  constructor(m: string, code: number) {
    super(m);
    this.name = "HttpError";
    this.code = code;
  }
}

class DbError extends Error {
  code: number;
  constructor(m: string, code: number) {
    super(m);
    this.name = "DbError";
    this.code = code;
  }
}

function route(which: number): string {
  try {
    if (which === 1) {
      throw new HttpError("gone", 410);
    }
    if (which === 2) {
      throw new Error("plain");
    }
    throw new DbError("deadlock", 40001);
  } catch (e: HttpError) {
    return "http:" + e.code.toString();
  } catch (e) {
    return "general:" + e.name;
  }
}

function main(): void {
  assert(route(1) === "http:410", "matching arm wins");
  assert(route(2) === "general:Error", "plain Error falls to the general arm");
  assert(route(3) === "general:DbError", "same-shape sibling falls to the general arm");
}
