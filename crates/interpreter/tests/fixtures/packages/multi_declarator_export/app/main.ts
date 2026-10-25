import { WIDTH, HEIGHT, label, count } from "@test/decls";

function main(): void {
  assert(WIDTH === 3, "the first declarator is exported");
  assert(HEIGHT === 6, "a later declarator is exported and reads an earlier one");
  assert(label === "box" && count === 0, "both `let` declarators are exported");
}
