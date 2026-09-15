// A `return` inside a later catch arm still runs this try's `finally` first.
class FirstError extends Error {
  constructor() {
    super("first");
    this.name = "FirstError";
  }
}

class SecondError extends Error {
  constructor() {
    super("second");
    this.name = "SecondError";
  }
}

let trail = "";

function inner(): number {
  try {
    throw new SecondError();
  } catch (e: FirstError) {
    return 1;
  } catch (e: SecondError) {
    trail = trail + "arm;";
    return 2;
  } finally {
    trail = trail + "fin;";
  }
}

function main(): void {
  const result = inner();
  assert(result === 2, "second arm's return value preserved");
  assert(trail === "arm;fin;", "finally ran after the arm body, before returning");
}
