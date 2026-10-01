import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { films, introduction, videoSource } from '../src/lib/videos.ts';
import { readChapters } from '../src/lib/agent-docs.ts';

test('unpublished and planned films never produce a production player', () => {
  for (const film of films) assert.equal(videoSource(film), undefined);
  const preview = '/docs/_video-preview/introduction.mp4';
  assert.equal(videoSource(introduction, true, preview), preview);
  assert.equal(videoSource(introduction, false, preview), undefined);
  assert.equal(videoSource(introduction, true, 'https://example.com/movie.mp4'), undefined);
  assert.equal(videoSource({ status: 'awaiting-publication', src: 'https://example.com/movie.mp4' }), undefined);
  assert.equal(videoSource({ status: 'published', src: 'https://example.com/movie.mp4' }), 'https://example.com/movie.mp4');
});

test('completed film has valid canonical, transcript and caption destinations', async () => {
  const chapters = await readChapters();
  const paths = chapters.map((chapter) => `/docs/${chapter.slug}/`);
  assert.ok(paths.includes(introduction.canonicalPath!));
  assert.ok(paths.includes(introduction.transcriptPath!));
  assert.equal(introduction.durationSeconds, 90.688);
  assert.equal(introduction.sha256, 'd6d505ba502b08ff13274c704df27f0fe1006571f1cf489598a9a5e370a682e1');
  assert.equal(new Set(films.map((film) => film.id)).size, films.length);
  const captions = await readFile(new URL('../public/videos/code-execution-introduction.en.vtt', import.meta.url), 'utf8');
  assert.ok(captions.startsWith('WEBVTT\n'));
  assert.ok(captions.includes("Consider an agent used by a small business."));
  assert.ok(captions.includes('agent stack.'));
  const transcript = chapters.find((chapter) => chapter.slug === 'videos/code-execution-introduction')!.body;
  const cues = captions.trim().split(/\n\n/).slice(1);
  let previousEnd = 0;
  const seconds = (stamp: string) => stamp.split(':').reduce((sum, part) => sum * 60 + Number(part), 0);
  for (const cue of cues) {
    const [timing, ...lines] = cue.split('\n');
    const [start, end] = timing.split(' --> ').map(seconds);
    assert.ok(start >= previousEnd && end > start && end <= 90.688, timing);
    previousEnd = end;
    assert.ok(transcript.includes(lines.join(' ')), lines.join(' '));
  }
});
