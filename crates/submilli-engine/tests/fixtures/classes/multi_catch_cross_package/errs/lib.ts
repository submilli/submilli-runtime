export class ParseError extends Error {
  line: number;
  constructor(message: string, line: number) {
    super(message);
    this.name = "ParseError";
    this.line = line;
  }
}

export class IoError extends Error {
  path: string;
  constructor(message: string, path: string) {
    super(message);
    this.name = "IoError";
    this.path = path;
  }
}

export function failParse(): void {
  throw new ParseError("bad token", 3);
}

export function failIo(): void {
  throw new IoError("disk gone", "/dev/sda");
}
