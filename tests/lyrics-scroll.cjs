const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');
const assert = require('node:assert/strict');
const widget = readFileSync(resolve(__dirname, '../src/views/WidgetIsland.vue'), 'utf8');
const scroll = ts.transpile(readFileSync(resolve(__dirname, '../src/features/lyrics/scroll.ts'), 'utf8')).replace('export ', '');
const extract = name => {
  const start = widget.indexOf(`const ${name} =`);
  return widget.slice(start, widget.indexOf('\n});', start) + 4);
};
const context = {
  exports: {},
  computed: fn => fn(),
  lyricLayout: { value: { textWidth: 400, viewportWidth: 200, words: [{ left: 160, width: 40 }, { left: 220, width: 60 }] } },
  activeLyricWordIndex: { value: 0 }, lyricWordProgress: { value: 1 },
  currentLyricLine: { value: { startMs: 0, endMs: 4000, words: [{ startMs: 0, endMs: 2000 }, { startMs: 2000, endMs: 4000 }] } }, lyricFrame: { value: { lineProgress: 0.5 } },
};
const offset = () => runInNewContext(`${scroll}\n${ts.transpile(extract('timedLyricScroll'))}\ntimedLyricScroll`, context);
const before = offset();
context.activeLyricWordIndex.value = 1;
context.lyricWordProgress.value = 0;
context.lyricFrame.value.lineProgress += 1 / 240;
assert.ok(Math.abs(offset() - before) < 3, 'Changing words must not jump the scrolling text');
context.currentLyricLine.value.endMs = 10000;
context.lyricFrame.value.lineProgress = 0.4;
assert.equal(offset(), 200, 'Trailing silence must not delay reaching the final sung word');
context.currentLyricLine.value.words = [];
context.lyricFrame.value.nextLine = { startMs: 3000 };
context.lyricFrame.value.lineProgress = 0.3;
assert.equal(offset(), 200, 'Overlapping next cue must shorten the scroll deadline');
context.lyricFrame.value.nextLine = null;

const start = widget.indexOf('const calculateScroll =');
const calculation = widget.slice(start, widget.indexOf('\n};', start) + 3);
Object.assign(context, {
  textInnerRef: { value: { scrollWidth: 400, querySelectorAll: () => [] } },
  maskBoxRef: { value: { clientWidth: 200 } }, isMusicExpanded: { value: false },
  isSizeAnimating: false, collapsedMaskWidth: 0, scrollDist: { value: 0 },
  scrollDuration: { value: '' }, isVideoPlayer: { value: false },
  getCurrentLineRemainingDuration: () => 4000,
});
runInNewContext(`${ts.transpile(calculation)}\ncalculateScroll();`, context);
context.lyricFrame.value.lineProgress = 1;
const textRight = 400 - offset();
assert.ok(textRight <= 188, 'Final glyph must stop before the 10px fade with breathing room');
assert.ok(400 - context.scrollDist.value <= 188, 'Fallback text must also clear the fade');
console.log('PASS: continuous word transitions and unobscured final text for timed/fallback lyrics.');
