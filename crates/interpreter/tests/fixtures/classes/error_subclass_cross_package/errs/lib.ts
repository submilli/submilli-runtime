/** A package-declared Error subclass. */
export class ParseError extends Error {
  line: number;
  constructor(message: string, line: number) {
    super(message);
    this.name = "ParseError";
    this.line = line;
  }
}

/** Throws a ParseError across the package boundary. */
export function parseOrThrow(text: string): string {
  if (text === "") {
    throw new ParseError("empty input", 1);
  }
  return text;
}
