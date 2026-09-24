let text: string | null = "before";
let count: number | null = 3;
function clearText(): boolean { text = null; return true; }
function clearCount(): boolean { count = null; return false; }
function main(): void {
  let caught = false;
  try {
    if (text !== null && clearText()) { text.length; }
  } catch (error: TypeError) { caught = true; }
  assert(caught, "&& guard mutation must throw TypeError");
  if (count === null || clearCount()) { assert(false); }
  else { assert(count + 1 === 1, "arithmetic converts the actual null value"); }
}
