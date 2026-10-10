function main(): void {
  const two = 2;
  let value = 4;
  value += two;
  value -= two;
  value *= two;
  value /= two;
  value **= two;
  value %= 7;
  assert(value === 2, "numeric compound operators accept literals");
  const suffix = "b";
  let text = "a";
  text += suffix;
  assert(text === "ab", "string compound addition accepts literals");
  const object = { value: 1 };
  object.value += two;
  const values = [1];
  values[0] += two;
  assert(object.value === 3 && values[0] === 3, "field and index writes");
}
