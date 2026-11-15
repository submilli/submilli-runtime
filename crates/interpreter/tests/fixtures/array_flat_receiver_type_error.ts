// expect-error: `flat` expects a receiver of type `string`
function main(): void {
  [[1]].flat<string, 1>();
}
