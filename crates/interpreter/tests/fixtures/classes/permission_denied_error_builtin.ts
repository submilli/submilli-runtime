class GatewayDenied extends PermissionDeniedError {
  constructor(message: string, caller: string, capability: string, reason: string) {
    super(message, caller, capability, reason);
    this.name = "GatewayDenied";
  }
}

function main(): void {
  // Constructible, with the inherited and own fields populated.
  const e = new PermissionDeniedError("denied", "main", "fs.read", "policy says no");
  assert(e.message === "denied", "message field");
  assert(e.name === "PermissionDeniedError", "name field");
  assert(e.caller === "main", "caller field");
  assert(e.capability === "fs.read", "capability field");
  assert(e.reason === "policy says no", "reason field");
  assert(e instanceof PermissionDeniedError, "instanceof own class");
  assert(e instanceof Error, "instanceof parent");
  assert(Error.isError(e), "Error.isError sees the subclass");
  assert(e.toString() === "PermissionDeniedError: denied", "toString");

  // Sibling built-in subclasses are distinct (checked through the shared
  // Error type — direct sibling instanceof is a static always-false error).
  const range: Error = new RangeError("out of range");
  assert(!(range instanceof PermissionDeniedError), "RangeError is not PermissionDeniedError");
  const base = new Error("plain");
  assert(!(base instanceof PermissionDeniedError), "base Error is not PermissionDeniedError");

  // Typed catch filters: the PermissionDeniedError arm binds one.
  let caught = "";
  try {
    throw new PermissionDeniedError("thrown", "main", "test.op", "no");
  } catch (e: PermissionDeniedError) {
    caught = e.name + ":" + e.capability;
  }
  assert(caught === "PermissionDeniedError:test.op", "typed catch binds the subclass");

  // A base Error skips the PermissionDeniedError arm.
  let arm = "";
  try {
    throw new Error("base");
  } catch (e: PermissionDeniedError) {
    arm = "denied";
  } catch (e) {
    arm = "base:" + e.name;
  }
  assert(arm === "base:Error", "base Error skips the PermissionDeniedError arm");

  // User subclasses chain through PermissionDeniedError to Error.
  const custom = new GatewayDenied("gw blocked", "main", "http.get", "blocked");
  assert(custom instanceof GatewayDenied, "instanceof own class");
  assert(custom instanceof PermissionDeniedError, "instanceof PermissionDeniedError parent");
  assert(custom instanceof Error, "instanceof Error root");
  assert(custom.name === "GatewayDenied", "subclass name");
  assert(custom.capability === "http.get", "subclass inherits capability field");
}
