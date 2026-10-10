interface OptionalValue { value?: string | null }
function describe(value: OptionalValue): string {
  if ("value" in value && value.value !== undefined && value.value !== null) {
    return value.value;
  }
  return "missing value";
}
function main(): void {
  const missing: OptionalValue = {};
  const explicit: OptionalValue = { value: undefined };
  assert(missing.value === undefined, "absence reads undefined");
  assert(!("value" in missing), "absent key is not present");
  assert("value" in explicit, "explicit undefined preserves key presence");
  assert(explicit.value === undefined, "presence does not imply a defined value");
  assert(describe(explicit) === "missing value", "guard undefined after presence");
  missing.value = undefined;
  assert("value" in missing, "writing undefined creates the key");
  missing.value = null;
  assert(missing.value === null, "explicit nullable field preserves null");
  missing.value = "ready";
  assert(describe(missing) === "ready", "present defined value narrows");
}
