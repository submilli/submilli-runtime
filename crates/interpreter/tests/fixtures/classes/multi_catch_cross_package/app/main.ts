import { ParseError, IoError, failParse, failIo } from "@test/errs";

function route(io: boolean): string {
  // IoError appears only in the second arm's annotation — its class must
  // still be reconstructed in this package for the dispatch test.
  try {
    if (io) {
      failIo();
    } else {
      failParse();
    }
    return "none";
  } catch (e: ParseError) {
    return "parse:" + e.line.toString();
  } catch (e: IoError) {
    return "io:" + e.path;
  }
}

function main(): void {
  assert(route(false) === "parse:3", "first arm binds the imported subclass");
  assert(route(true) === "io:/dev/sda", "second arm binds the other imported subclass");
}
