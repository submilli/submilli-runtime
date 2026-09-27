// The tools a model needs to write and run programs on submilli-server, built
// on the server's HTTP API for the Vercel AI SDK.

import { tool, type ToolSet } from "ai";
import { z } from "zod";

export interface SessionOptions {
  /** Base URL of submilli-server. */
  server: string;
  /** The registered blueprint every program in this session runs under. */
  blueprint: string;
  /** Values for the blueprint's variables, from your application's own state. */
  variables?: Record<string, string>;
}

export interface Session {
  /** Hand these to `generateText` or `streamText`. */
  tools: ToolSet;
  /** Ends the session and wipes its files. */
  close(): Promise<void>;
}

/** The descriptions the server publishes for its MCP tools, fetched over HTTP. */
interface Prompt {
  prompt: string;
  tools: {
    search: string;
    docs: string;
    builtins_list: string;
    builtins_docs: string;
    last_run: string;
  };
}

export async function openSession(options: SessionOptions): Promise<Session> {
  const blueprint = `${options.server}/v1/blueprints/${encodeURIComponent(options.blueprint)}`;
  const describe: Prompt = await json(await fetch(`${blueprint}/prompt`));

  const { session_id } = await json(
    await fetch(`${options.server}/v1/sessions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ blueprint: options.blueprint, variables: options.variables ?? {} }),
    }),
  );
  const session = `${options.server}/v1/sessions/${session_id}`;

  return {
    tools: {
      // The model supplies the code and nothing else. The blueprint and the
      // variables were fixed above, where the model cannot reach them.
      submilli__typescript__execute: tool({
        description: describe.prompt,
        inputSchema: z.object({ code: z.string() }),
        execute: async ({ code }) => {
          const run = await json(
            await fetch(`${session}/execute`, {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ code }),
            }),
          );
          // A denial or a compile error arrives here as `error`, for the model to read.
          return { result: run.result, console: run.console, error: run.error };
        },
      }),
      submilli__typescript__last_run: tool({
        description: describe.tools.last_run,
        inputSchema: z.object({}),
        execute: async () => json(await fetch(`${session}/last-run`)),
      }),
      submilli__typescript__packages__search: tool({
        description: describe.tools.search,
        inputSchema: z.object({ query: z.string().default("") }),
        execute: async ({ query }) =>
          json(await fetch(`${blueprint}/packages/search?${new URLSearchParams({ q: query })}`)),
      }),
      submilli__typescript__packages__docs: tool({
        description: describe.tools.docs,
        inputSchema: z.object({ name: z.string() }),
        execute: async ({ name }) => {
          const docs = await fetch(`${blueprint}/packages/docs?${new URLSearchParams({ name })}`);
          return docs.text();
        },
      }),
      submilli__typescript__builtins__list: tool({
        description: describe.tools.builtins_list,
        inputSchema: z.object({}),
        execute: async () => json(await fetch(`${blueprint}/builtins`)),
      }),
      submilli__typescript__builtins__docs: tool({
        description: describe.tools.builtins_docs,
        inputSchema: z.object({ names: z.array(z.string()) }),
        execute: async ({ names }) => {
          const query = new URLSearchParams(names.map((name) => ["name", name]));
          return json(await fetch(`${blueprint}/builtins/docs?${query}`));
        },
      }),
    },
    close: async () => {
      await fetch(session, { method: "DELETE" });
    },
  };
}

async function json(response: Response): Promise<any> {
  const body = await response.json();
  if (!response.ok) {
    throw new Error(`submilli-server answered ${response.status}: ${body.message ?? body.error}`);
  }
  return body;
}
