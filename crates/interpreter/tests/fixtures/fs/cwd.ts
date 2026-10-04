import { cwd, writeText, readText } from "submilli:fs";

function main(): void {
  assert(cwd() === "/", "default working directory");
  writeText("cwd.txt", "hello");
  assert(readText("/cwd.txt") === "hello", "relative and absolute paths agree");
}
