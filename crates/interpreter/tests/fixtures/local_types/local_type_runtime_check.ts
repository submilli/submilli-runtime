// A failed runtime check names a local type by its source name.

function parseRow(json: string): string {
  interface Row { id: number; }
  try {
    const row = JSON.parse(json) as Row;
    return `${row.id}`;
  } catch (e) {
    return e instanceof Error ? e.message : "?";
  }
}

function main(): void {
  assert(parseRow("{\"id\": 4}") === "4", "a matching value");
  const message = parseRow("{\"id\": \"x\"}");
  assert(message.includes("Row") && !message.includes("%"), message);
}
