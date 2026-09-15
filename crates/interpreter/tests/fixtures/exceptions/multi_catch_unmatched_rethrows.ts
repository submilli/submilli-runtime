// When no arm matches, the original error re-raises to the enclosing handler.
class RedError extends Error {
  constructor() {
    super("red");
    this.name = "RedError";
  }
}

class BlueError extends Error {
  constructor() {
    super("blue");
    this.name = "BlueError";
  }
}

class GreenError extends Error {
  constructor() {
    super("green");
    this.name = "GreenError";
  }
}

function main(): void {
  let trail = "";
  try {
    try {
      throw new GreenError();
    } catch (e: RedError) {
      trail = trail + "red;";
    } catch (e: BlueError) {
      trail = trail + "blue;";
    }
  } catch (e) {
    trail = trail + "outer:" + e.name;
  }
  assert(trail === "outer:GreenError", "unmatched error re-raises the original");
}
