import { get } from "submilli:variables";

function main(): void {
  let denied = false;
  try {
    get("BROWSER_STORE_NAME");
  } catch (error: unknown) {
    denied = error instanceof PermissionDeniedError;
  }
  assert(denied, "configuration is available only to packages");
}
