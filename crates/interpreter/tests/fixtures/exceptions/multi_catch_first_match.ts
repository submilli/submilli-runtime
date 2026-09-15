// Multiple catch clauses: the thrown value dispatches to the first arm whose
// class matches, and the binding has that arm's declared type.
class ParseError extends Error {
  line: number;
  constructor(m: string, line: number) {
    super(m);
    this.name = "ParseError";
    this.line = line;
  }
}

class IoError extends Error {
  path: string;
  constructor(m: string, path: string) {
    super(m);
    this.name = "IoError";
    this.path = path;
  }
}

function poke(which: number): string {
  try {
    if (which === 1) {
      throw new ParseError("bad token", 12);
    }
    throw new IoError("missing", "/tmp/x");
  } catch (e: ParseError) {
    return "parse:" + e.line.toString();
  } catch (e: IoError) {
    return "io:" + e.path;
  }
}

function main(): void {
  assert(poke(1) === "parse:12", "ParseError lands in the ParseError arm");
  assert(poke(2) === "io:/tmp/x", "IoError lands in the IoError arm");
}
