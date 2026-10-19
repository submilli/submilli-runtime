// SUB-1754: every built-in error constructor's message is optional, as in tsc.
function main(): void {
  const uri = new URIError();
  const reference = new ReferenceError();
  assert(uri.message === "" && uri.name === "URIError");
  assert(reference.message === "" && reference.name === "ReferenceError");
  assert(new URIError("bad").message === "bad");
}
