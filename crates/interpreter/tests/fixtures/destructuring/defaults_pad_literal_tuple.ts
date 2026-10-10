// An array pattern longer than the literal it takes apart is accepted when
// every slot past the literal's end has a default or is a hole, as tsc pads
// the tuple with `undefined`. A defaulted slot past the end takes its
// default's type.
function main(): void {
  const [a, b = 2] = [1];
  assert(a === 1 && b === 2, "a constant default");
  const [first, second = first + 1] = [5];
  assert(first === 5 && second === 6, "a default reading an earlier slot");
  const [count, label = "none"] = [3];
  const text: string = label;
  assert(count === 3 && text === "none", "a default of another type than the literal's");
  const [n, word, flag = true] = [1, "x"];
  const w: string = word;
  assert(n === 1 && w === "x" && flag, "a literal of mixed element types");
  const [head, , tail = 9] = [4];
  assert(head === 4 && tail === 9, "a hole past the end");
  let [start, step = 1] = [0];
  step = 2;
  assert(start + step === 2, "a `let` pattern");
  const [o, shape = {}] = [1];
  const [l, list = []] = [1];
  const [base, read = () => base] = [4];
  assert(JSON.stringify(shape) === "{}" && list.length === 0 && read() === 4, "defaults that need no hint");
  const [only, ...rest] = [7];
  assert(only === 7 && rest.length === 0, "a rest element past the end");
}
