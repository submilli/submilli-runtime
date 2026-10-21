// Response decoding for the Jina client's JSON endpoints. Internal — `src/lib.ts`
// re-exports the result types and calls the decoders; programs never import
// this module.

/** Structured page returned by `readJson`. */
export interface ReaderResult {
    /** Page title (empty when Jina reports none). */
    title: string;
    /** Page description / summary (empty when absent). */
    description: string;
    /** Final URL the Reader resolved. */
    url: string;
    /** LLM-friendly page body, as markdown. */
    content: string;
    /** Tokens Jina billed for the read (0 when not reported). */
    tokens: number;
}
/** One structured search hit returned by `searchJson`. */
export interface SearchResult {
    /** Result title (empty when absent). */
    title: string;
    /** Result description / snippet (empty when absent). */
    description: string;
    /** Result URL. */
    url: string;
    /** LLM-friendly result body, as markdown (empty when absent). */
    content: string;
    /** Tokens Jina billed for this result (0 when not reported). */
    tokens: number;
}
/**
 * Decode a Reader JSON response (`readJson`) without network access. Fields Jina
 * omits or reports as `null` become empty strings and zero tokens.
 * @param body Raw JSON response body from the Reader endpoint.
 * @returns The normalized page; throws `Error` if the body is not a Reader envelope.
 */
export function normalizeReaderJson(body: string): ReaderResult {
    try {
        const data = (JSON.parse(body) as ReaderEnvelope).data;
        return {
            title: data.title ?? "",
            description: data.description ?? "",
            url: data.url,
            content: data.content ?? "",
            tokens: data.usage?.tokens ?? 0,
        };
    } catch (cause) {
        throw invalidResponse();
    }
}

/**
 * Decode a Search JSON response (`searchJson`) without network access. A `null`
 * result list means no hits; per-result fields Jina omits or reports as `null`
 * become empty strings and zero tokens.
 * @param body Raw JSON response body from the Search endpoint.
 * @returns One entry per hit, in Jina's order; throws `Error` if the body is not a Search envelope.
 */
export function normalizeSearchJson(body: string): SearchResult[] {
    try {
        const envelope = JSON.parse(body) as SearchEnvelope;
        const results: SearchResult[] = [];
        const data = envelope.data;
        if (data === null) return results;
        for (const item of data) {
            results.push({
                title: item.title ?? "",
                description: item.description ?? "",
                url: item.url,
                content: item.content ?? "",
                tokens: item.usage?.tokens ?? 0,
            });
        }
        return results;
    } catch (cause) {
        throw invalidResponse();
    }
}
interface ReaderEnvelope {
    data: ReaderData;
}

interface ReaderData {
    title?: string | null;
    description?: string | null;
    url: string;
    content?: string | null;
    usage?: Usage | null;
}

interface SearchEnvelope {
    data: SearchData[] | null;
}

interface SearchData {
    title?: string | null;
    description?: string | null;
    url: string;
    content?: string | null;
    usage?: Usage | null;
}

interface Usage {
    tokens?: number | null;
}
function invalidResponse(): Error {
    return new Error("Jina returned an invalid response");
}
