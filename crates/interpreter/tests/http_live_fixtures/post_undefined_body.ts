import { post } from "submilli:http";

function main(): void {
  assert(post("{{BASE}}/post-empty", undefined).status === 200);
  assert(post("{{BASE}}/post-empty").status === 200);
}
