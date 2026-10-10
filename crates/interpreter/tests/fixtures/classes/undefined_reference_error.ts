class MissingBindingError extends ReferenceError {
  constructor(message: string) {
    super(message);
    this.name = "MissingBindingError";
  }
}

function main(): void {
  const error = new ReferenceError("binding is not initialized");
  assert(error.name === "ReferenceError");
  assert(error.message === "binding is not initialized");
  assert(error instanceof ReferenceError);
  assert(error instanceof Error);
  assert(Error.isError(error));
  assert(error.toString() === "ReferenceError: binding is not initialized");

  const base: Error = error;
  assert(!(base instanceof TypeError));
  assert(!(base instanceof RangeError));
  assert(!(base instanceof SyntaxError));

  let caught = "";
  try {
    throw error;
  } catch (e: ReferenceError) {
    caught = e.name + ":" + e.message;
  }
  assert(caught === "ReferenceError:binding is not initialized");

  let selected = "";
  try {
    throw new Error("ordinary error");
  } catch (e: ReferenceError) {
    selected = "reference";
  } catch (e) {
    selected = e.name;
  }
  assert(selected === "Error");

  const custom = new MissingBindingError("custom binding");
  assert(custom.name === "MissingBindingError");
  assert(custom.message === "custom binding");
  assert(custom instanceof MissingBindingError);
  assert(custom instanceof ReferenceError);
  assert(custom instanceof Error);
}
