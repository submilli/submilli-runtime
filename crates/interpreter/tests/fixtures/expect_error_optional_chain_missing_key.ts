// expect-error: field `missing` does not exist
type Headers = { "content-type": string };

function main(): void {
  const h: Headers | null = { "content-type": "x" };
  const v = h?.["missing"];
}
