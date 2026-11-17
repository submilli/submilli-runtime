interface Keys { "雪"?: number; ""?: number; "get p"?: number; "1"?: number }
function main(): number {
  const keys: Keys = { "雪": 1, "": 2, "get p": 3, "1": 4 };
  if (!delete keys["雪"] || "雪" in keys) return 1;
  if (!delete keys[""] || "" in keys) return 2;
  if (!delete keys["get p"] || "get p" in keys) return 3;
  if (!delete keys["1"] || "1" in keys) return 4;
  if (Object.keys(keys).length !== 0 || JSON.stringify(keys) !== "{}") return 5;
  return 0;
}
