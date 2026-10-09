type LabeledOptional = [a: number, b?: string];
type Optional = [number, string?];

function main(): void {
  const labeled: LabeledOptional = [1];
  const unlabeled: Optional = [2, "present"];
  const absent: Optional = [3, undefined];
  assert(labeled[0] === 1, "required labeled element remains present");
  assert(labeled[1] === undefined, "omitted optional element reads undefined");
  assert(unlabeled[1] === "present", "present optional element keeps its value");
  assert(absent[1] === undefined, "optional elements accept explicit undefined");
}
