let object: { value: string | null } = { value: "hello" };
let nested: { mid: { leaf: string | null } | null } = { mid: { leaf: "nested" } };
function clear(): boolean { object.value = null; return true; }
function main(): void {
  assert((object.value !== null ? object.value.length : 0) === 5);
  assert(object.value !== null && object.value.length === 5);
  if (object.value !== null) {
    assert((object.value === null ? "missing" : object.value) === "hello");
  }
  if (nested.mid !== null && nested.mid.leaf !== null) {
    assert(nested.mid.leaf === "nested");
  }
  let caught = false;
  try {
    if (object.value !== null && clear()) { object.value.length; }
  } catch (error: TypeError) { caught = true; }
  assert(caught);
}
