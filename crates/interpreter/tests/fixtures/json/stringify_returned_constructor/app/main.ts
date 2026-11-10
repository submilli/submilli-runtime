import { getNum } from "@test/provider";

function main(): void {
  const num = getNum();
  const optional: string | undefined = JSON.stringify(num);
  assert(optional === undefined, "returned constructor in an optional place");
  let caught = false;
  try {
    const text: string = JSON.stringify(getNum());
    assert(false, text);
  } catch (e) {
    assert(e.name === "MissingValueError", e.message);
    caught = true;
  }
  assert(caught, "returned constructor in a string place throws");
}
