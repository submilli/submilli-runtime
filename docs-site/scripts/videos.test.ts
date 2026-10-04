import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { films, introduction, videoSource, challenges, previewPath } from '../src/lib/videos.ts';
import { readChapters } from '../src/lib/agent-docs.ts';

test('production players require an explicit published version and public source', () => {
  for (const film of films) {
    if (film.status !== 'published') assert.equal(videoSource(film), undefined);
    else assert.ok(videoSource(film), `${film.id} published source must be valid`);
  }
  assert.equal(videoSource({ status: 'published', src: 'https://example.com/movie.mp4' }), undefined);
  for (const src of ['http://example.com/movie.mp4', 'https://localhost/movie.mp4', 'https://127.0.0.1/movie.mp4', 'https://0.0.0.0/movie.mp4', 'https://255.255.255.255/movie.mp4', 'https://[::1]/movie.mp4', 'https://[::]/movie.mp4', 'https://[0:0:0:0:0:0:0:1]/movie.mp4', 'https://[0:0:0:0:0:0:0:0]/movie.mp4', 'https://[fc00::1]/movie.mp4', 'https://[fd12::1]/movie.mp4', 'https://[fe80::1]/movie.mp4', 'https://[::ffff:192.168.0.1]/movie.mp4', 'https://192.168.0.1/movie.mp4', 'https://user:pass@example.com/movie.mp4', 'not a URL']) {
    assert.equal(videoSource({ status: 'published', src, publishedVersion: 'v1' }), undefined);
  }
  const preview = '/docs/_video-preview/introduction.mp4';
  assert.equal(videoSource(introduction, true, preview), preview);
  assert.equal(videoSource(introduction, false, preview), videoSource(introduction));
  assert.equal(videoSource(introduction, true, 'https://example.com/movie.mp4'), videoSource(introduction));
  assert.equal(videoSource({ status: 'awaiting-publication', src: 'https://example.com/movie.mp4' }), undefined);
  assert.equal(videoSource({ status: 'published', src: 'https://example.com/movie.mp4', publishedVersion: 'v1' }), 'https://example.com/movie.mp4');
});

test('completed film has valid canonical, transcript and caption destinations', async () => {
  const chapters = await readChapters();
  const paths = chapters.map((chapter) => `/docs/${chapter.slug}/`);
  assert.ok(paths.includes(introduction.canonicalPath!));
  assert.ok(paths.includes(introduction.transcriptPath!));
  const poster = await readFile(new URL('../public/videos/code-execution-introduction.svg', import.meta.url), 'utf8');
  assert.ok(poster.includes('<svg'));
  assert.equal(introduction.posterPath, '/docs/videos/code-execution-introduction.svg');
  assert.equal(introduction.embedPath, '/docs/videos/embed/code-execution-introduction/');
  if (introduction.exportFile === 'editor-export-1790685929055.mp4') {
    assert.equal(introduction.durationSeconds, 90.688);
    assert.equal(introduction.sha256, 'd6d505ba502b08ff13274c704df27f0fe1006571f1cf489598a9a5e370a682e1');
  }
  assert.equal(new Set(films.map((film) => film.id)).size, films.length);
  const captions = await readFile(new URL('../public/videos/code-execution-introduction.en.vtt', import.meta.url), 'utf8');
  assert.ok(captions.startsWith('WEBVTT\n'));
  assert.ok(captions.replace(/\s+/g, ' ').includes("Consider an agent used by a small business."));
  assert.ok(captions.includes('agent stack.'));
  const transcript = chapters.find((chapter) => chapter.slug === 'videos/code-execution-introduction')!.body;
  const cues = captions.trim().split(/\n\n/).slice(1);
  assert.ok(cues.length > 0);
  const narration = [...transcript.matchAll(/## \d+:\d+ — [^\n]+\n\n([\s\S]*?)(?=\n## |$)/g)]
    .map((match) => match[1].trim()).join(' ');
  assert.equal(cues.map((cue) => cue.split('\n').slice(1).join(' ')).join(' '), narration);
  let previousEnd = 0;
  const seconds = (stamp: string) => stamp.split(':').reduce((sum, part) => sum * 60 + Number(part), 0);
  for (const cue of cues) {
    const [timing, ...lines] = cue.split('\n');
    const [start, end] = timing.split(' --> ').map(seconds);
    assert.ok(start >= previousEnd && end > start && end <= introduction.durationSeconds!, timing);
    assert.ok(end - start >= 1 && end - start <= 7, 'Readable cue duration: ' + timing);
    assert.ok(lines.length <= 2 && lines.every((line) => line.length <= 42), 'At most two 42-character lines');
    assert.ok(lines.join(' ').length / (end - start) <= 20, 'Caption reading speed stays below 20 characters/second');
    previousEnd = end;
    assert.ok(transcript.includes(lines.join(' ')), lines.join(' '));
  }
});

test('local preview overrides stay bound to their film identities', () => {
  const intro = '/docs/_video-preview/introduction.mp4';
  const challenge = '/docs/_video-preview/challenges.mp4';
  assert.equal(previewPath(introduction, intro, challenge), intro);
  assert.equal(previewPath(challenges, intro, challenge), challenge);
  assert.equal(previewPath(films.find(film => film.id === 'helps')!, intro, challenge), '');
  assert.equal(challenges.canonicalPath, '/docs/why/#this-code-is-a-stranger');
  assert.equal(challenges.embedPath, '/docs/videos/embed/challenges/');
});

test('published films include their matching release metadata and caption assets', async () => {
  assert.deepEqual(films.map(film => film.id), ['code-execution-introduction', 'challenges', 'helps', 'using', 'works']);
  for (const film of films) {
    const poster = await readFile(new URL(`../public/videos/${film.id}.svg`, import.meta.url), 'utf8');
    assert.ok(poster.includes('<svg'));
    if (!videoSource(film)) continue;
    assert.ok(film.durationSeconds && film.durationSeconds > 0, `${film.id} duration`);
    assert.match(film.sha256 ?? '', /^[a-f0-9]{64}$/, `${film.id} SHA-256`);
    assert.equal(film.publishedVersion, film.sha256, `${film.id} immutable version`);
    assert.ok(film.captionsPath?.startsWith('/docs/videos/'), `${film.id} captions`);
    const captionFile = new URL(`../public/${film.captionsPath!.slice('/docs/'.length)}`, import.meta.url);
    assert.ok((await readFile(captionFile, 'utf8')).startsWith('WEBVTT\n'), `${film.id} WebVTT asset`);
  }
});
