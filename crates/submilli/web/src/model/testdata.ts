// Recorded answers from a scratch playground, put together as the store would hand them
// to the page. Only the tests import this.
import type { Compare, Explain, PageData, PackageSource, SessionList, Show, Status, StoredEvent, Changes } from "../api/types";
import { sourceKey } from "../api/types";

const json = import.meta.glob("../../test/fixtures/*.json", { eager: true, import: "default" });
const raw = import.meta.glob("../../test/fixtures/*.sse", { eager: true, import: "default", query: "?raw" });

export function fixture<T>(name: string): T {
  const found = json[`../../test/fixtures/${name}`];
  if (found === undefined) throw new Error(`no fixture ${name}`);
  // A fresh copy, so a test that changes its data leaves the others alone.
  return structuredClone(found) as T;
}

/** The stored events of a recorded feed, in the order it sent them. */
export function feed(name: string): StoredEvent[] {
  const text = raw[`../../test/fixtures/${name}`] as string | undefined;
  if (text === undefined) throw new Error(`no feed ${name}`);
  const events: StoredEvent[] = [];
  for (const block of text.split("\n\n")) {
    const data = /^data: (.*)$/m.exec(block);
    if (!data) continue;
    const parsed = JSON.parse(data[1]) as StoredEvent & { kind?: string };
    if (parsed.body) events.push(parsed);
  }
  return events;
}

export const CHAT_SESSION = "5e550000-0000-4000-8000-000000000004";

/** The chat session with everything loaded: its runs, their tests and rerun, decisions, versions. */
export function chatSession(): PageData {
  const runs: Record<number, Show> = {};
  for (const n of [1, 2, 3, 4, 5, 6, 7]) runs[n] = fixture<Show>(`show-${n}.json`);
  const explains: Record<string, Explain> = {};
  for (const ref of ["1.1", "1.2", "2.1", "4.1", "4.2", "5.1", "5.2", "6.1", "6.2", "7.1", "7.2"]) {
    explains[ref] = fixture<Explain>(`explain-${ref}.json`);
  }
  const compares: Record<number, Compare> = {
    5: fixture<Compare>("compare-2-5.json"),
    6: fixture<Compare>("compare-2-6.json"),
    7: fixture<Compare>("compare-1-7.json"),
  };
  const v1 = fixture<Changes>("changes-v1.json").versions[0];
  const v2 = fixture<Changes>("changes-v2.json").versions[0];
  const pkg = fixture<PackageSource>("model-package-source.json");
  return {
    status: fixture<Status>("status.json"),
    sessions: fixture<SessionList>("sessions.json").sessions,
    sessionId: CHAT_SESSION,
    events: feed("feed-session-4.sse"),
    feedEnded: false,
    runs,
    running: {},
    explains,
    compares,
    versions: { 1: v1, 2: v2 },
    sources: { [sourceKey(pkg.package, pkg.module)]: pkg },
  };
}

/** The chat session plus two tests that stopped at a call with no recorded reply. */
export function withStoppedTests(): PageData {
  const data = chatSession();
  data.runs[8] = fixture<Show>("model-show-8-stopped.json");
  data.runs[9] = fixture<Show>("model-show-9-stopped.json");
  data.compares[8] = fixture<Compare>("model-compare-2-8.json");
  data.compares[9] = fixture<Compare>("model-compare-1-9.json");
  data.runs[2].reruns.push({ run: 8, kind: "test", blueprint_version: "2", outcome: { kind: "stopped" } });
  data.runs[1].reruns.push({ run: 9, kind: "test", blueprint_version: "2", outcome: { kind: "stopped" } });
  return data;
}

/** A session whose one program makes plain calls, with a package search and a replaced chat turn. */
export function plainSession(): PageData {
  const s = fixture<{ session: string; events: StoredEvent[]; show: Show }>("model-session-5.json");
  const data = chatSession();
  data.sessionId = s.session;
  data.events = s.events;
  data.runs = { 10: s.show };
  data.sessions = [
    {
      session: s.session,
      started_at: "2026-10-13T13:27:00Z",
      started_at_micros: 1791898020000000,
      sources: ["chat"],
      variables: { customerId: "cus_northwind" },
      model: "anthropic:claude-sonnet-5-5",
      run_count: 1,
      runs: [10],
      denied: false,
      cancelled: 0,
      rejected: 0,
      failed: 0,
      open: false,
      follow_ups: 0,
      asks: 1,
      untrusted: { task: "What is my total?" },
    },
  ];
  return data;
}
