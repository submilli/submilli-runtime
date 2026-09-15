// Divergence pin: JS splices capture-group values into the split result
// ("hello".split(/(l)/) is ["he", "l", "", "l", "o"]); here captures are not
// inserted (prelude regex doc-comment documents the divergence), so the result
// is the plain three-way split.

function main(): void {
  const parts = "hello".split(/(l)/);
  assertCompareArray(parts, ["he", "", "o"], "captures are not spliced into the result");
}
