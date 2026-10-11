// expect-error: expected
function invalid({ kind }: { kind: "A" | "B" }): void {
  kind = "C";
}
function main(): void {}
