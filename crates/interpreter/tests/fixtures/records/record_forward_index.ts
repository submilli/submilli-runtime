interface Dict { [key: string]: { x: number }; value: Value; }
interface Container { dict: { [key: string]: { x: number }; value: ZValue }; }
interface ClassContainer { dict: { [key: string]: { x: number }; value: Value }; }
interface ZValue { x: number; }
class Value { x: number = 1; }
function main(): void {
  const dict: Dict = { value: new Value() };
  assert(dict.value.x === 1);
  const nested: Container = { dict: { value: { x: 2 } } };
  assert(nested.dict.value.x === 2);
  const classes: ClassContainer = { dict: { value: new Value() } };
  assert(classes.dict.value.x === 1);
}
