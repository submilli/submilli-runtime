function main(): void {
  let message: string = "";

  try {
    throw new Error("boom");
  } catch (e) {
    message = e.message;
  }

  assert(message === "boom", "untyped catch binding reads as Error");
}
