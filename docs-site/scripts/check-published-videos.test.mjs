import test from 'node:test';
import assert from 'node:assert/strict';
import { checkPublishedVideos, parseOrigin, PRODUCTION_ORIGIN } from './check-published-videos.mjs';

const film = {
  id: 'packages',
  status: 'published',
  embedUrl: `${PRODUCTION_ORIGIN}/embed/packages/`,
};

const manifest = {
  film: 'packages',
  durationSeconds: 12.5,
  media: '/assets/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.mp4',
  captions: '/assets/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.vtt',
  transcript: '/assets/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.txt',
};

function response(body, options = {}) {
  return new Response(body, options);
}

function hostedFetch(overrides = {}) {
  const calls = [];
  const fetchImpl = async (input, init = {}) => {
    const url = String(input);
    calls.push({ url, init });
    if (overrides[url]) return overrides[url](init);
    if (url.endsWith('/player.js')) return response('console.log("player");', { headers: { 'content-type': 'application/javascript' } });
    if (url.endsWith('/embed/packages/')) return response('<video data-film="packages"></video><script src="/player.js"></script>');
    if (url.endsWith('/films/packages/current.json')) return response(JSON.stringify(manifest), { headers: { 'content-type': 'application/json' } });
    if (url.endsWith('.mp4')) return response(new Uint8Array([0]), { status: 206, headers: { 'content-type': 'video/mp4' } });
    if (url.endsWith('.vtt')) return response('WEBVTT\n\n00:00.000 --> 00:01.000\nHello');
    if (url.endsWith('.txt')) return response('A useful transcript.');
    throw new Error(`unexpected request ${url}`);
  };
  return { calls, fetchImpl };
}

test('published video check validates every hosted dependency and uses a one-byte media range', async () => {
  const { calls, fetchImpl } = hostedFetch();
  assert.deepEqual(await checkPublishedVideos({ films: [film], fetchImpl }), ['packages']);
  const mediaCall = calls.find((call) => call.url.endsWith('.mp4'));
  assert.equal(mediaCall.init.headers.Range, 'bytes=0-0');
});

test('the check rejects a 200 HTML not-found page from an embed host', async () => {
  const { fetchImpl } = hostedFetch({
    [`${PRODUCTION_ORIGIN}/embed/packages/`]: async () => response('<html><title>Not Found</title></html>'),
  });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl }), /packages: embed returned HTML without the expected player markup/);
});

test('the check rejects a missing or empty player script', async () => {
  const missing = hostedFetch({ [`${PRODUCTION_ORIGIN}/player.js`]: async () => response('Not Found', { status: 404 }) });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: missing.fetchImpl }), /packages: player returned HTTP 404/);
  const empty = hostedFetch({ [`${PRODUCTION_ORIGIN}/player.js`]: async () => response('', { headers: { 'content-type': 'application/javascript' } }) });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: empty.fetchImpl }), /packages: player is empty/);
});

test('the check rejects a manifest for another film or an unusable duration', async () => {
  const { fetchImpl } = hostedFetch({
    [`${PRODUCTION_ORIGIN}/films/packages/current.json`]: async () => response(JSON.stringify({ ...manifest, film: 'other', durationSeconds: 0 })),
  });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl }), /packages: manifest film/);

  const invalidDuration = hostedFetch({
    [`${PRODUCTION_ORIGIN}/films/packages/current.json`]: async () => response(JSON.stringify({ ...manifest, durationSeconds: 0 })),
  });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: invalidDuration.fetchImpl }), /packages: manifest durationSeconds must be positive and finite/);
});

test('the check rejects unsafe asset paths and invalid captions', async () => {
  const { fetchImpl } = hostedFetch({
    [`${PRODUCTION_ORIGIN}/films/packages/current.json`]: async () => response(JSON.stringify({ ...manifest, media: 'https://evil.example/video.mp4' })),
  });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl }), /packages: manifest media must be a content-hashed/);

  const invalidCaptions = hostedFetch({
    [`${PRODUCTION_ORIGIN}/assets/${'b'.repeat(64)}.vtt`]: async () => response('not captions'),
  });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: invalidCaptions.fetchImpl }), /packages: captions are not a WEBVTT/);
});

test('the check reports missing media, captions, and transcripts and rejects empty media', async () => {
  const missingMedia = hostedFetch({ [`${PRODUCTION_ORIGIN}/assets/${'a'.repeat(64)}.mp4`]: async () => response('', { status: 404, headers: { 'content-type': 'text/html' } }) });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: missingMedia.fetchImpl }), /packages: media returned HTTP 404/);
  const missingCaptions = hostedFetch({ [`${PRODUCTION_ORIGIN}/assets/${'b'.repeat(64)}.vtt`]: async () => response('', { status: 404 }) });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: missingCaptions.fetchImpl }), /packages: captions returned HTTP 404/);
  const missingTranscript = hostedFetch({ [`${PRODUCTION_ORIGIN}/assets/${'c'.repeat(64)}.txt`]: async () => response('', { status: 404 }) });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: missingTranscript.fetchImpl }), /packages: transcript returned HTTP 404/);
  const emptyMedia = hostedFetch({ [`${PRODUCTION_ORIGIN}/assets/${'a'.repeat(64)}.mp4`]: async () => response(new Uint8Array(), { status: 206, headers: { 'content-type': 'video/mp4', 'content-length': '0' } }) });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl: emptyMedia.fetchImpl }), /packages: media returned an empty body/);
});

test('local origin override is restricted to loopback', () => {
  assert.equal(parseOrigin('http://127.0.0.1:4282').origin, 'http://127.0.0.1:4282');
  assert.equal(parseOrigin(PRODUCTION_ORIGIN).origin, PRODUCTION_ORIGIN);
  assert.throws(() => parseOrigin('https://example.test'), /only allowed for loopback/);
});


test('a stalled media body times out and releases the stream', async () => {
  const { fetchImpl } = hostedFetch({
    [`${PRODUCTION_ORIGIN}${manifest.media}`]: async ({ signal }) => response(new ReadableStream({
      start(controller) {
        signal.addEventListener('abort', () => controller.error(new DOMException('Aborted', 'AbortError')), { once: true });
      },
    }), { headers: { 'content-type': 'video/mp4' } }),
  });
  await assert.rejects(checkPublishedVideos({ films: [film], fetchImpl, timeoutMs: 20 }), /media request failed.*timed out/);
});
