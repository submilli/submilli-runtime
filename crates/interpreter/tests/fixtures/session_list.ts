import session from "submilli:session";

function keysOf(page: session.Page): string {
  let joined = "";
  for (const entry of page.entries) {
    joined = joined + entry.key + ",";
  }
  return joined;
}

function main(): void {
  // Written out of order; `list` must report them in UTF-16 code-unit order.
  session.set("triage/b", 2);
  session.set("triage/a", 1);
  session.set("notes/x", "x");
  session.set("triage/ab", 3);

  const all = session.list("", 100, null);
  assert(keysOf(all) === "notes/x,triage/a,triage/ab,triage/b,", "code-unit order");
  assert(all.nextCursor === null, "an exhausted keyspace has no cursor");

  // Prefix matches exact code units, with no path semantics: "triage/a" is a
  // prefix of "triage/ab", not a directory.
  const prefixed = session.list("triage/a", 100, null);
  assert(keysOf(prefixed) === "triage/a,triage/ab,", "prefix is exact code units");

  // A prefix that matches nothing yields an empty page and no cursor.
  const none = session.list("zzz", 100, null);
  assert(none.entries.length === 0, "no matches yields no entries");
  assert(none.nextCursor === null, "no matches yields no cursor");

  // `sizeBytes` is the stored value's serialized size, not key + value. `1`
  // serializes to one code unit; a two-code-unit key would double the count if
  // the key were included.
  assert(all.entries[1].key === "triage/a", "entry order is stable");
  assert(all.entries[1].sizeBytes === 2, "sizeBytes is the value payload only");
  session.set("triage/a", "abcd");
  const resized = session.list("triage/a", 100, null);
  assert(resized.entries[0].sizeBytes === 12, "\"abcd\" serializes to 6 code units");

  // Pagination: a limit smaller than the match count returns a cursor that
  // resumes strictly after the last emitted key.
  const first = session.list("triage/", 2, null);
  assert(keysOf(first) === "triage/a,triage/ab,", "first page respects the limit");
  assert(first.nextCursor !== null, "a truncated page carries a cursor");

  const second = session.list("triage/", 2, first.nextCursor);
  assert(keysOf(second) === "triage/b,", "the cursor resumes after the last key");
  assert(second.nextCursor === null, "the final page has no cursor");

  // A page filled exactly to the limit cannot know whether more matches follow,
  // so it hands back a cursor; that cursor's page is empty and final. Paging
  // until the cursor is null therefore terminates without re-reporting a key.
  const exact = session.list("triage/", 3, null);
  assert(keysOf(exact) === "triage/a,triage/ab,triage/b,", "a full page holds every match");
  assert(exact.nextCursor !== null, "a page filled to the limit still offers a cursor");
  const past = session.list("triage/", 3, exact.nextCursor);
  assert(past.entries.length === 0, "the page past the last match is empty");
  assert(past.nextCursor === null, "and ends the walk");

  // Limit bounds: 1 and 1000 are accepted, 0 and 1001 are not.
  assert(session.list("", 1, null).entries.length === 1, "limit 1 is accepted");
  assert(session.list("", 1000, null).entries.length === 4, "limit 1000 is accepted");

  let rejectedZero = false;
  try {
    session.list("", 0, null);
  } catch (e: Error) {
    rejectedZero = true;
  }
  assert(rejectedZero, "limit 0 is rejected");

  let rejectedOver = false;
  try {
    session.list("", 1001, null);
  } catch (e: Error) {
    rejectedOver = true;
  }
  assert(rejectedOver, "limit 1001 is rejected");

  // A cursor is opaque: it must not be the key it resumes after.
  const cursor = first.nextCursor;
  assert(cursor !== null, "cursor present");
  const opaque = cursor as string;
  assert(opaque !== "triage/ab", "the cursor is not the bare key");

  // A cursor minted for one prefix is refused by another.
  let crossPrefix = false;
  try {
    session.list("notes/", 2, first.nextCursor);
  } catch (e: Error) {
    crossPrefix = true;
  }
  assert(crossPrefix, "a cursor from a different prefix is refused");

  // So is a cursor that was never minted here.
  let malformed = false;
  try {
    session.list("triage/", 2, "not-a-cursor");
  } catch (e: Error) {
    malformed = true;
  }
  assert(malformed, "a malformed cursor is refused");

  // A page cut short by the scan bound rather than by the match count still
  // carries a cursor: the caller must page again rather than conclude the
  // keyspace is exhausted. 600 non-matching keys sort before "zz/" and exceed
  // the scan bound, so the first page for that prefix is empty yet unfinished.
  for (let i = 0; i < 600; i = i + 1) {
    session.set("pad/" + i.toString(), 0);
  }
  const bounded = session.list("zz/", 10, null);
  assert(bounded.entries.length === 0, "the scan bound cut the page short");
  assert(bounded.nextCursor !== null, "a bound-limited short page still pages on");

  session.set("zz/found", 7);
  let walked = 0;
  let next: string | null = null;
  let found = false;
  while (walked < 20) {
    const page = session.list("zz/", 10, next);
    for (const entry of page.entries) {
      if (entry.key === "zz/found") {
        found = true;
      }
    }
    next = page.nextCursor;
    if (next === null) {
      break;
    }
    walked = walked + 1;
  }
  assert(found, "paging to exhaustion reaches keys past the scan bound");
  assert(next === null, "the walk ended because the keyspace was exhausted");
}
