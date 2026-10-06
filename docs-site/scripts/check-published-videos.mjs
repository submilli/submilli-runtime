import registry from '../src/data/videos.json' with { type: 'json' };

export const PRODUCTION_ORIGIN = 'https://submilli-videos.onrender.com';
const DEFAULT_TIMEOUT_MS = 8000;
const HASHED_ASSET = /^\/assets\/[0-9a-f]{64}\.(mp4|vtt|txt)$/;

function fail(film, message) {
  throw new Error(`Published video check failed for ${film.id}: ${message}`);
}

function normalizedOrigin(value) {
  const url = new URL(value);
  if (url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new Error(`Video origin must be an origin URL: ${value}`);
  }
  return url;
}

export function parseOrigin(value = PRODUCTION_ORIGIN) {
  const origin = normalizedOrigin(value);
  if (!['http:', 'https:'].includes(origin.protocol)) {
    throw new Error(`Video origin must use HTTP or HTTPS: ${origin.href}`);
  }
  if (origin.origin !== PRODUCTION_ORIGIN) {
    const isLoopback = origin.hostname === 'localhost'
      || origin.hostname === '127.0.0.1'
      || origin.hostname === '[::1]'
      || origin.hostname === '::1';
    if (!isLoopback) throw new Error(`--origin is only allowed for loopback local checks: ${origin.href}`);
  }
  return origin;
}

async function readText(fetchImpl, url, film, timeoutMs, label) {
  try {
    const { response, text } = await requestText(fetchImpl, url, {}, film, timeoutMs);
    if (!response.ok) fail(film, `${label} returned HTTP ${response.status} (${url})`);
    return text;
  } catch (error) {
    if (error.message?.startsWith('Published video check failed')) throw error;
    fail(film, `${label} body could not be read (${error.message})`);
  }
}

async function requestText(fetchImpl, url, init, film, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetchImpl(url, { ...init, signal: controller.signal });
    return { response, text: await response.text() };
  } catch (error) {
    const reason = error?.name === 'AbortError' ? `timed out after ${timeoutMs}ms` : error.message;
    fail(film, `${url} request failed: ${reason}`);
  } finally {
    clearTimeout(timer);
  }
}

async function checkPlayer(origin, fetchImpl, film, timeoutMs) {
  const playerUrl = new URL('/player.js', origin);
  const { response, text } = await requestText(fetchImpl, playerUrl, {}, film, timeoutMs);
  if (!response.ok) fail(film, `player returned HTTP ${response.status} (${playerUrl})`);
  const contentType = response.headers.get('content-type')?.toLowerCase().split(';', 1)[0];
  if (!['application/javascript', 'text/javascript', 'application/ecmascript', 'text/ecmascript', 'application/x-javascript'].includes(contentType)) {
    fail(film, `player did not identify itself as JavaScript (${playerUrl})`);
  }
  if (!text.trim()) fail(film, `player is empty (${playerUrl})`);
}

function sameOriginAsset(origin, value, film, field) {
  if (typeof value !== 'string' || !HASHED_ASSET.test(value)) {
    fail(film, `${field} must be a content-hashed /assets path`);
  }
  const url = new URL(value, origin);
  if (url.origin !== origin.origin || url.pathname !== value || url.search || url.hash) {
    fail(film, `${field} must be a same-origin relative asset path`);
  }
  return url;
}

function assertManifest(manifest, film, origin) {
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest)) fail(film, 'manifest is not an object');
  if (manifest.film !== new URL(film.embedUrl).pathname.split('/')[2]) {
    fail(film, `manifest film ${JSON.stringify(manifest.film)} does not match the embed slug`);
  }
  if (!Number.isFinite(manifest.durationSeconds) || manifest.durationSeconds <= 0) {
    fail(film, 'manifest durationSeconds must be positive and finite');
  }
  sameOriginAsset(origin, manifest.media, film, 'manifest media');
  sameOriginAsset(origin, manifest.captions, film, 'manifest captions');
  sameOriginAsset(origin, manifest.transcript, film, 'manifest transcript');
}

async function checkFilm(film, origin, fetchImpl, timeoutMs) {
  let embed;
  try {
    embed = new URL(film.embedUrl);
  } catch {
    fail(film, `embed URL is invalid: ${film.embedUrl}`);
  }
  if (embed.origin !== PRODUCTION_ORIGIN) fail(film, `embed URL must use ${PRODUCTION_ORIGIN}`);
  if (!/^\/embed\/[a-z0-9-]+\/$/.test(embed.pathname)) fail(film, `embed URL has an invalid path: ${embed.pathname}`);

  const embedUrl = new URL(embed.pathname, origin);
  const { response: embedResponse, text: embedMarkup } = await requestText(fetchImpl, embedUrl, {}, film, timeoutMs);
  if (!embedResponse.ok) fail(film, `embed returned HTTP ${embedResponse.status} (${embedUrl})`);
  const slug = embed.pathname.split('/')[2];
  if (!/<video\b/i.test(embedMarkup) || !new RegExp(`data-film=["']${slug}["']`).test(embedMarkup)
    || !/src=["']\/player\.js["']/.test(embedMarkup)) {
    fail(film, 'embed returned HTML without the expected player markup');
  }

  const manifestUrl = new URL(`/films/${slug}/current.json`, origin);
  const { response: manifestResponse, text: manifestText } = await requestText(fetchImpl, manifestUrl, {}, film, timeoutMs);
  if (!manifestResponse.ok) fail(film, `manifest returned HTTP ${manifestResponse.status} (${manifestUrl})`);
  let manifest;
  try {
    manifest = JSON.parse(manifestText);
  } catch (error) {
    fail(film, `manifest is not valid JSON (${error.message})`);
  }
  assertManifest(manifest, film, origin);

  await checkMedia(new URL(manifest.media, origin), fetchImpl, film, timeoutMs);

  const captions = await readText(fetchImpl, new URL(manifest.captions, origin), film, timeoutMs, 'captions');
  if (!/^WEBVTT(?:\s|$)/.test(captions.trim())) fail(film, 'captions are not a WEBVTT file');
  const transcript = await readText(fetchImpl, new URL(manifest.transcript, origin), film, timeoutMs, 'transcript');
  if (!transcript.trim()) fail(film, 'transcript is empty');
}

async function checkMedia(url, fetchImpl, film, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  let response;
  let reader;
  try {
    response = await fetchImpl(url, { headers: { Range: 'bytes=0-0' }, signal: controller.signal });
    if (![200, 206].includes(response.status)) fail(film, `media returned HTTP ${response.status} (${url})`);
    if (!response.headers.get('content-type')?.toLowerCase().startsWith('video/mp4')) {
      fail(film, `media did not identify itself as video/mp4 (${url})`);
    }
    reader = response.body?.getReader();
    const chunk = await reader?.read();
    if (!chunk?.value?.byteLength) fail(film, `media returned an empty body (${url})`);
  } catch (error) {
    if (error.message?.startsWith('Published video check failed')) throw error;
    fail(film, `media request failed (${url}): ${controller.signal.aborted ? `timed out after ${timeoutMs}ms` : error.message}`);
  } finally {
    controller.abort();
    clearTimeout(timer);
    try {
      if (reader) await reader.cancel();
      else await response?.body?.cancel();
    } catch { /* The request may already have aborted its body. */ }
  }
}

export async function checkPublishedVideos({ films = registry.films, origin = PRODUCTION_ORIGIN, fetchImpl = globalThis.fetch, timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
  if (typeof fetchImpl !== 'function') throw new Error('Published video check requires fetch');
  const parsedOrigin = parseOrigin(origin);
  const published = films.filter((film) => film.status === 'published');
  if (!published.length) throw new Error('Published video check found no published films');
  await checkPlayer(parsedOrigin, fetchImpl, published[0], timeoutMs);
  for (const film of published) await checkFilm(film, parsedOrigin, fetchImpl, timeoutMs);
  return published.map((film) => film.id);
}

if (import.meta.url === new URL(process.argv[1], `file://${process.cwd()}/`).href) {
  const hasOriginFlag = process.argv[2] === '--origin';
  const originArg = hasOriginFlag ? process.argv[3] : undefined;
  if (process.argv.length > 2 && process.argv[2] !== '--origin') {
    throw new Error('Usage: node check-published-videos.mjs [--origin http://127.0.0.1:4282]');
  }
  if (hasOriginFlag && !originArg) throw new Error('Usage: node check-published-videos.mjs [--origin http://127.0.0.1:4282]');
  const origin = parseOrigin(originArg);
  if (hasOriginFlag && origin.origin === PRODUCTION_ORIGIN) {
    throw new Error('--origin is only for loopback local checks; production is checked by default');
  }
  checkPublishedVideos({ origin })
    .then((films) => console.log(`Verified hosted video readiness for ${films.length} published film${films.length === 1 ? '' : 's'}.`))
    .catch((error) => {
      console.error(error.message);
      process.exitCode = 1;
    });
}
