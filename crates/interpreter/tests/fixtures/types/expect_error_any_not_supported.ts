// `any` is not a supported type — use a concrete type, an interface, or `unknown`.
// expect-error: `any` is not supported

function main(): void {
  const x: any = 1;
}
