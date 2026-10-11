class OptionalValue {
  constructor(public value: number | undefined = 1) {}
}
class UndefinedValue {
  constructor(public value: undefined = undefined) {}
}
class InferredUndefinedValue {
  constructor(public value = undefined) {}
}
function main(): void {
  const optional = new OptionalValue();
  assert(optional.value === 1, "default initializes the property");
  optional.value = undefined;
  assert(optional.value === undefined, "property retains its declared undefined alternative");
  optional.value = 2;
  assert(optional.value === 2, "property retains its declared number alternative");
  const only = new UndefinedValue();
  assert(only.value === undefined, "undefined-only parameter property has a value carrier");
  only.value = undefined;
  assert("value" in only, "undefined property remains present");
  assert(new InferredUndefinedValue().value === undefined, "inferred undefined property remains undefined");
}
