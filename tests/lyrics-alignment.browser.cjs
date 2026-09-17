const { chromium } = require('playwright');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const assert = require('node:assert/strict');
const widget = readFileSync(resolve(__dirname, '../src/views/WidgetIsland.vue'), 'utf8');
const css = [...widget.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map(m => m[1]).join('\n');
(async () => {
  const browser = await chromium.launch({ channel: 'msedge', headless: true });
  try {
    const page = await browser.newPage();
    for (const zoom of [1, 1.5, 2]) for (const center of [false, true]) for (const long of [false, true]) {
      await page.setContent(`<style>${css} html { zoom: ${zoom}; }</style>
        <div class="${center ? 'lyrics-centered' : ''}" style="position:relative;width:200px;height:40px">
          <span class="lyric-render-text"><span class="scroll-inner">${long ? 'Lời bài hát rất dài cần cuộn để đọc đầy đủ cả câu' : 'Lời bài hát'}</span></span>
        </div>`);
      const b = await page.evaluate(() => {
        const outer = document.querySelector('.lyric-render-text').getBoundingClientRect();
        const inner = document.querySelector('.scroll-inner').getBoundingClientRect();
        return { left: inner.left - outer.left, free: outer.width - inner.width };
      });
      assert.ok(Math.abs(b.left - (center && !long ? b.free / 2 : 0)) < 1,
        JSON.stringify({ zoom, center, long, b }));
    }
    console.log('PASS: left/center alignment and long-line start visibility at 100%, 150%, 200%.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
