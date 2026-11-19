// What requests carry: the access token, a secret bound by the harness. Where
// they go is the `orgUrl` build setting (`src/config.ts`).

import secrets from "submilli:secrets";
import { validationError } from "./errors";

// The access token, sent as `Authorization: Bearer`.
export function accessToken(): string {
    const token = secrets.get("SALESFORCE_ACCESS_TOKEN");
    if (token === undefined) throw validationError("missing_token", "SALESFORCE_ACCESS_TOKEN is not bound");
    return token;
}
