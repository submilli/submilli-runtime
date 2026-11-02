function main(): void {
  const done: boolean = Math.random() < 2;
  assert("Done: " + done === "Done: true");
  assert(false + "!" === "false!");
  let s = "";
  s += done;
  assert(s === "true");
}
