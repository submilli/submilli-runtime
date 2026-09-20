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
  caught = false;
  try {
    if (count === null || clearCount()) {} else { count + 1; }
  } catch (error: TypeError) { caught = true; }
  assert(caught, "|| guard mutation must throw TypeError");
}
