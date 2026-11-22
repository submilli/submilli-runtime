// What requests carry: the access token, a secret bound by the harness. Where
// they go is the `accountUrl` build setting (`src/config.ts`).

import secrets from "submilli:secrets";
import { validationError } from "./errors";

// The OAuth access token, sent as `Authorization: Bearer`.
export function accessToken(): string {
    const token = secrets.get("ZENDESK_ACCESS_TOKEN");
    if (token === undefined) throw validationError("missing_token", "ZENDESK_ACCESS_TOKEN is not bound");
    return token;
}
