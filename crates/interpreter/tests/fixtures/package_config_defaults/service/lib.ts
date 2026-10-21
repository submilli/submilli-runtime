import { get } from "submilli:http";
import { baseUrl, timeoutMs, enabled, negativeZero } from "./config";
export function endpoint(): string { return baseUrl; }
export function timeout(): number { return timeoutMs; }
export function active(): boolean { return enabled; }
export function zero(): number { return negativeZero; }
export function fetchText(): string { return get(baseUrl).body; }
