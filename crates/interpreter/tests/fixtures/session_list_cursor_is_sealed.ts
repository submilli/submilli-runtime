// A cursor resumes after the last key the listing *considered*, which under a
// per-key `session.read` filter is routinely a key the caller was denied. So a
// cursor its holder could read would hand back exactly the names the filter
// withheld. This walks the keyspace one key at a time — the shape that made the
// disclosure maximal — and requires the key to be unrecoverable from every
// cursor, by spelling and by decoding the base64url payload in guest code.
import session from "submilli:session";

const ALPHA = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

function decodeBase64Url(text: string): number[] {
  const bytes: number[] = [];
  let acc = 0;
  let bits = 0;
  for (let i = 0; i < text.length; i = i + 1) {
    const idx = ALPHA.indexOf(text.substring(i, i + 1));
    if (idx >= 0) {
      acc = acc * 64 + idx;
      bits = bits + 6;
      if (bits >= 8) {
        bits = bits - 8;
        const shift = Math.pow(2, bits);
        const byte = Math.floor(acc / shift);
        bytes.push(byte);
        acc = acc - byte * shift;
      }
    }
  }
  return bytes;
}

// Read the payload as UTF-16BE from every offset, which is how the cleartext
// layout gave the key up: the key sat at a fixed offset under the digest.
function containsAsUtf16(bytes: number[], needle: string): boolean {
  for (let start = 0; start < bytes.length; start = start + 1) {
    let text = "";
    let j = start;
    while (j + 1 < bytes.length) {
      text = text + String.fromCharCode(bytes[j] * 256 + bytes[j + 1]);
      j = j + 2;
    }
    if (text.indexOf(needle) >= 0) {
      return true;
    }
  }
  return false;
}

function main(): void {
  session.set("a-visible", 1);
  session.set("m-SECRET-KEY-NAME", 2);
  session.set("z-visible", 3);

  // Page one key at a time so a cursor is minted after each, including after
  // the key in the middle.
  const cursors: string[] = [];
  let cursor: string | undefined;
  let guard = 0;
  while (guard < 10) {
    guard = guard + 1;
    const page = session.list("", 1, cursor);
    const next = page.nextCursor;
    if (next === undefined) {
      break;
    }
    cursors.push(next);
    cursor = next;
  }
  assert(cursors.length > 1, "the walk minted a cursor after each key");

  for (const c of cursors) {
    assert(c.indexOf("SECRET-KEY-NAME") < 0, "a cursor must not spell the key");
    assert(
      !containsAsUtf16(decodeBase64Url(c), "SECRET-KEY-NAME"),
      "a cursor must not decode to the key it resumes after",
    );
  }

  // Sealing must not cost correctness: the cursors still page the keyspace.
  let seen = 0;
  let next: string | undefined;
  let steps = 0;
  while (steps < 10) {
    steps = steps + 1;
    const page = session.list("", 1, next);
    seen = seen + page.entries.length;
    next = page.nextCursor;
    if (next === undefined) {
      break;
    }
  }
  assert(seen === 3, "every key is still reachable by paging");

  // A cursor the runtime did not issue is refused rather than read as a key.
  let forged = false;
  try {
    session.list("", 1, "AQAAAAAAAAAAAG0AUwBFAEMAUgBFAFQ");
  } catch (e: Error) {
    forged = true;
  }
  assert(forged, "a hand-built cursor is refused");
}
