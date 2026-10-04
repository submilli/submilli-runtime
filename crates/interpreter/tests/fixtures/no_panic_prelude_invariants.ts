function check(condition: boolean): void {
  if (!condition) throw new Error("prelude invariant regression");
}

function main(): string {
  check(decodeURIComponent("é%F0%9F%98%80%00") === "é😀\u0000");
  check(decodeURI("%2f%3F%23%F0%9F%98%80") === "%2f%3F%23😀");
  check(encodeURIComponent("é😀") === "%C3%A9%F0%9F%98%80");
  check(new RegExp("é\\d😀", "").test("é7😀"));
  check(new RegExp("[é\\d]", "").test("7"));
  check(new RegExp("\\é", "").test("é"));
  check((123.456).toPrecision(4) === "123.5");
  check((0.00000125).toPrecision(2) === "0.0000013");
  check((255).toString(16) === "ff");
  check((10.5).toString(2) === "1010.1");
  let caught = false;
  try { (1).toPrecision(0); } catch (error) { caught = error instanceof RangeError; }
  check(caught);
  return "ok";
}
