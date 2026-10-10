function optionalValue(): number | undefined {
  return undefined;
}

class InferredOptionalValue {
  constructor(public value = optionalValue()) {}
}

class InferredNumberValue {
  constructor(public value = 1) {}
}

function main(): void {
  const optional = new InferredOptionalValue();
  assert(optional.value === undefined, "property preserves the inferred initializer result");
  optional.value = 2;
  assert(optional.value === 2);
  optional.value = undefined;
  assert(optional.value === undefined, "undefined remains a valid inferred property write");
  assert(new InferredOptionalValue(3).value === 3, "explicit argument initializes the property");
  const number = new InferredNumberValue(undefined);
  const initial: number = number.value;
  assert(initial === 1, "omission does not widen a definite initializer's property type");
  number.value = 4;
  assert(number.value === 4, "literal initializer widens to number");
}
