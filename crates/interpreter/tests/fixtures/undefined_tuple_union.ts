type OptionalValue = [number, string?] | [number, boolean?];
function read(value: OptionalValue): string | boolean | undefined { return value[1]; }
function optional(value: OptionalValue | null): string | boolean | undefined { return value?.[1]; }
function main(): void {
  assert(read([1]) === undefined, "union preserves an omitted optional tuple element");
  assert(read([1, "present"]) === "present");
  assert(read([1, true]) === true);
  assert(optional([1]) === undefined && optional(null) === undefined);
}
